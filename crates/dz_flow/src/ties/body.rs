//! Structured, per-function tie analysis (spec §6.7).
//!
//! A reference remembers both its logical target and the tie keeping its address valid.
//! For `&xs[i]` these differ: the target is `xs[*]`, but its address is in `xs@items`.
//! A later view of the element's owned allocation follows the target, not that address.

use std::collections::{HashMap, HashSet};

use dz_check::{FnSig, ItemSig, TypeTable};
use dz_hir::ty::{self, Ty};
use dz_hir::{self as hir, BlockId, Body, BodyOwner, Callee, ExprId, LocalId, StmtId};
use dz_project::ModuleId;
use dz_resolve::{Resolution, SymbolId};
use dz_span::Span;

use super::{Bindings, Mismatch, ParameterTie, Place, ReturnTie, Step};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum KillKind {
    Call,
    Assign,
    Move,
    Drop,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BodyKill {
    pub place: Place,
    /// The operation in this body, never the callee's declaration.
    pub span: Span,
    pub kind: KillKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvalidUse {
    pub span: Span,
    pub tie: Place,
    pub killed: BodyKill,
    /// A loop's next pass over its container, rather than a use the program writes: the
    /// iterator `for` keeps reads the memory a call in the body killed (spec §6.7).
    pub next_pass: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvalidReturn {
    pub span: Span,
    pub expected: Place,
    pub actual: Place,
}

/// A boundary this implementation cannot yet model. In particular, an incomplete body
/// must never produce a fix claiming its clause can be removed or narrowed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Incomplete {
    pub span: Span,
    pub reason: &'static str,
}

#[derive(Clone, Debug, Default)]
pub struct BodyAnalysis {
    pub kills: Vec<BodyKill>,
    pub invalid_uses: Vec<InvalidUse>,
    pub invalid_returns: Vec<InvalidReturn>,
    /// Only effects visible to callers, reduced to the narrowest set of paths.
    pub actual: Vec<Place>,
    /// In source-clause order, in the same coordinates as `actual`.
    pub declared: Vec<Place>,
    pub mismatches: Vec<Mismatch>,
    /// Where a kill may have been missed or widened, so the summary is not exact and the
    /// clause is not compared with it.
    pub incomplete: Vec<Incomplete>,
    /// Where a view could not be followed, so a use or a return went unchecked. This never
    /// hides a kill: every way a kill reaches unknown memory is `incomplete` instead.
    pub unchecked: Vec<Incomplete>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Origin {
    target: Place,
    tie: Place,
    /// An `in value` or elided result may point into any tie under `value`.
    broad: bool,
    /// The union or optional whose payload this points into. Any kill of that place ends
    /// it, since the variant may change, though a payload is inline (spec §6.7).
    payload: Option<Place>,
    dead: Option<BodyKill>,
}

/// How far a kill reaches. Dropping or moving a place ends every view into it. Writing it,
/// or a call's clause, moves only what the old value owned and changes payloads: a view of
/// a plain inline field still points at valid memory, as after `p.x += 1` (spec §6.7).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Reach {
    All,
    Owned,
}

impl KillKind {
    fn reach(self) -> Reach {
        match self {
            KillKind::Drop | KillKind::Move => Reach::All,
            KillKind::Assign | KillKind::Call => Reach::Owned,
        }
    }
}

impl Origin {
    fn at(place: Place) -> Self {
        Self { target: place.clone(), tie: place, broad: false, payload: None, dead: None }
    }

    fn ended_by(&self, kill: &Place, reach: Reach) -> bool {
        if self.payload.as_ref().is_some_and(|union| kill.covers(union)) {
            return true;
        }
        if self.broad && self.tie.covers(kill) {
            return true;
        }
        if !kill.covers(&self.tie) {
            return false;
        }
        // A broad view under the kill may point into what it owns.
        reach == Reach::All
            || self.broad
            || kill.steps.last() == Some(&Step::Storage)
            || self.tie.steps[kill.steps.len()..].contains(&Step::Storage)
    }

    fn field(&self, step: Step) -> Self {
        let target = self.target.followed_by(std::slice::from_ref(&step));
        // A field inside an array element still lives in the array's storage. For an
        // inline value, a sibling field must remain independent.
        let tie =
            if self.tie == self.target { self.tie.followed_by(&[step]) } else { self.tie.clone() };
        Self { target, tie, ..self.clone() }
    }

    /// A reference to the payload of the union or optional at this place.
    fn payload(&self) -> Self {
        Self { payload: Some(self.target.clone()), ..self.clone() }
    }
}

/// The place an expression denotes and the provenance of any view it produces are
/// separate. Reading a raw pointer field has a known place, but an unknown pointee.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Value {
    places: Vec<Origin>,
    views: Vec<Origin>,
    unknown: bool,
}

type State = HashMap<LocalId, Value>;

#[derive(Default)]
struct Loop {
    label: Option<u32>,
    scope_depth: usize,
    breaks: Vec<(State, Vec<Value>)>,
    continues: Vec<(State, Vec<Value>)>,
}

struct Analysis<'a> {
    body: &'a Body,
    table: &'a TypeTable,
    res: &'a Resolution,
    state: State,
    /// Values being evaluated are live even when they have no local binding.
    held: Vec<Value>,
    live: bool,
    loops: Vec<Loop>,
    /// Locals and deferred statements belonging to each active lexical scope.
    scopes: Vec<(Vec<LocalId>, Vec<StmtId>)>,
    result: BodyAnalysis,
    expected_return: ReturnTie,
    reported: HashSet<Span>,
}

/// Analyse one checked function, using only other functions' signatures. Findings are
/// available independently of enforcement so std and editor support can land together.
pub fn analyse_body(body: &Body, table: &TypeTable, res: &Resolution) -> BodyAnalysis {
    let mut cx = Analysis {
        body,
        table,
        res,
        state: State::new(),
        held: Vec::new(),
        live: true,
        loops: Vec::new(),
        scopes: Vec::new(),
        result: BodyAnalysis::default(),
        expected_return: ReturnTie::None,
        reported: HashSet::new(),
    };
    let signature = body_signature(body, table, res);
    let mut external = HashSet::new();
    for local in &body.params {
        let l = body.local(*local);
        if super::points(&l.ty) || l.name == "self" {
            let origin = Origin::at(Place::local(*local));
            cx.state.insert(*local, Value { views: vec![origin], ..Value::default() });
        }
    }
    if let Some(signature) = &signature {
        let bindings = body_bindings(body);
        if signature.sig.takes_self == Some(true)
            && !matches!(
                body.owner,
                BodyOwner::Ctor { .. } | BodyOwner::Copy(_) | BodyOwner::Drop(_)
            )
        {
            external.extend(bindings.receiver.iter().map(|p| p.local));
        }
        for (i, param) in signature.sig.params.iter().enumerate() {
            if super::mutable_input(&param.ty) {
                external.extend(bindings.params.get(i).into_iter().flatten().map(|p| p.local));
            }
        }
        let scope = signature.scope(table, res);
        match super::call_effects(&scope, &bindings) {
            Ok(effects) => {
                cx.expected_return = effects.result;
                cx.result.declared = effects.kills.into_iter().map(|k| k.place).collect();
                let offset = usize::from(signature.sig.takes_self.is_some());
                for (i, tie) in effects.parameter_ties.into_iter().enumerate() {
                    if let (ParameterTie::Known(place), Some(local)) =
                        (tie, body.params.get(i + offset))
                    {
                        // The input's exact address is unknown, but its annotation says
                        // which owner's kills can end it inside the body.
                        if let Some(value) = cx.state.get_mut(local) {
                            for origin in &mut value.views {
                                origin.tie = place.clone();
                                origin.broad = true;
                            }
                        }
                    }
                }
            }
            Err(_) => cx.incomplete(body.span, "the body's tie paths do not resolve"),
        }
    } else if !matches!(
        body.owner,
        BodyOwner::Const(_)
            | BodyOwner::Static(_)
            | BodyOwner::AssocConst { .. }
            | BodyOwner::FieldDefault { .. }
    ) {
        // An initialiser has no parameters and no clause, so nothing a caller sees can
        // die in it and there is no summary to be incomplete; its uses are still walked.
        cx.incomplete(body.span, "this body has no callable signature");
    }
    if let Some(init) = body.base_init {
        cx.expr(init);
    }
    cx.block(body.root);
    // By-value parameters are owned by this body and drop after its local scopes.
    for local in body.params.iter().rev() {
        let l = body.local(*local);
        if l.name != "self" && !super::points(&l.ty) {
            cx.kill(Place::local(*local), body.span, KillKind::Drop);
        }
    }
    cx.result.actual = super::minimal(
        &cx.result
            .kills
            .iter()
            .filter(|k| external.contains(&k.place.local))
            .map(|k| k.place.clone())
            .collect::<Vec<_>>(),
    );
    if let Some(signature) =
        signature.as_ref().filter(|s| s.sig.has_body && cx.result.incomplete.is_empty())
    {
        // Compared over the ties each path names, so `s@text` and `s@text@items` agree
        // where the text owns nothing else (§6.7).
        let scope = signature.scope(table, res);
        let ties = |places: &[Place]| {
            places
                .iter()
                .map(|p| super::ties(&scope, p, &body.local(p.local).ty))
                .collect::<Vec<_>>()
        };
        cx.result.mismatches =
            super::compare_ties(&ties(&cx.result.declared), &ties(&cx.result.actual));
    }
    cx.result
}

impl Analysis<'_> {
    fn targets(&self, expression: ExprId, value: &Value) -> Vec<Origin> {
        // An `own` pointer is not a view in a signature, but what it denotes is still
        // the memory behind it, never the field holding it.
        let ty = self.body.ty(expression);
        if super::points(ty) || matches!(ty, Ty::OwnPtr { .. }) {
            value.views.clone()
        } else {
            value.places.clone()
        }
    }

    fn field(&mut self, base: ExprId, index: usize) -> Value {
        let value = self.expr(base);
        // A payload's fields are read like a struct's (`case Circle(&c)`, then `c.r`).
        let field = match self.body.ty(base).peeled() {
            Ty::Named { sym, args } => self
                .table
                .struct_sig(*sym)
                .and_then(|s| s.fields.get(index))
                .map(|f| (*sym, args, f)),
            Ty::Variant(sym, variant, args) => match self.table.get(*sym) {
                Some(ItemSig::Union { variants, .. }) => variants
                    .get(*variant)
                    .and_then(|v| v.fields.get(index))
                    .map(|f| (*sym, args, f)),
                _ => None,
            },
            _ => None,
        };
        let Some((sym, args, field)) = field else {
            self.unchecked(self.body.expr(base).span, "field storage is not known");
            return Value::default();
        };
        let sym = &sym;
        let field_ty = self.table.field_types(*sym, args, std::slice::from_ref(field)).remove(0);
        let places = self
            .targets(base, &value)
            .iter()
            .map(|o| o.field(Step::Field(super::canonical_field(self.table, *sym, &field.name))))
            .collect::<Vec<_>>();
        let mut views = Vec::new();
        let mut unknown = false;
        if super::points(&field_ty) || matches!(field_ty, Ty::OwnPtr { .. }) {
            if field.tie.is_some() && matches!(field_ty, Ty::Ptr { .. } | Ty::OwnPtr { .. }) {
                views = places
                    .iter()
                    .map(|o| {
                        let place = o.target.followed_by(&[Step::Storage]);
                        Origin {
                            target: place.clone(),
                            tie: place,
                            broad: false,
                            // What a pointer owns is not inline in a payload: dropping
                            // the payload ends it as owned memory instead.
                            payload: None,
                            dead: o.dead.clone(),
                        }
                    })
                    .collect();
            } else {
                unknown = true;
            }
        }
        let places = places.into_iter().map(|o| self.canonical(o)).collect();
        let views = views.into_iter().map(|o| self.canonical(o)).collect();
        Value { places, views, unknown }
    }

    /// `origin` with its places spelt as the ties that name them ([`super::canonicalize`]):
    /// `m_data[i]` inside a `List` is `self[*]`, and a `SlotMap`'s second list its
    /// `@items`, as the first is.
    fn canonical(&self, mut origin: Origin) -> Origin {
        let canon = |p: &Place| {
            super::canonical_place(self.table, self.res, &self.body.local(p.local).ty, p)
        };
        origin.target = canon(&origin.target);
        origin.tie = canon(&origin.tie);
        origin
    }

    fn index_origins(&mut self, base: ExprId, value: &Value, span: Span) -> Vec<Origin> {
        let targets = self.targets(base, value);
        match self.body.ty(base).peeled_refs() {
            Ty::Named { .. } => {
                let Some(signature) = index_signature(self.body.ty(base), self.table, self.res)
                else {
                    self.unchecked(span, "the indexed element's storage is unknown");
                    return Vec::new();
                };
                let result = self.invoke(&signature, Some(value), &[], span);
                let mut elements = Vec::new();
                // Each possible receiver has its own result provenance. Keep its
                // logical element path even when the returned address is @items.
                for parent in &targets {
                    for origin in &result.views {
                        if parent.target.covers(&origin.target) {
                            let mut origin = origin.clone();
                            origin.target = parent.target.followed_by(&[Step::Elements]);
                            elements.push(origin);
                        }
                    }
                }
                elements
            }
            Ty::Ptr { .. } | Ty::OwnPtr { .. } => {
                // An element behind a tied pointer is the type's `[*]` where it declares
                // `tie [*] = m_data[*]`; its address stays valid as long as the storage.
                let elements: Vec<Origin> = targets
                    .iter()
                    .map(|o| {
                        let storage = o.target.steps.last() == Some(&Step::Storage);
                        let mut e = o.field(Step::Elements);
                        if storage {
                            e.tie = o.tie.clone();
                        }
                        self.canonical(e)
                    })
                    .collect();
                if elements.iter().any(|o| o.target.steps.contains(&Step::Storage)) {
                    self.incomplete(span, "the type declares no `tie [*]` for this pointer");
                } else if targets.iter().any(|o| o.target.steps.last() == Some(&Step::Storage)) {
                    // Uses are followed through `[*]` all the same; only the summary is not
                    // held to the clause. std's clauses over raw storage are written by hand
                    // and trusted (§6.7): `pop` moves an element out, which the checker
                    // would read as killing `self[*]`, and whether it should is still open
                    // (T-0107).
                    self.incomplete(span, "a body over a tied pointer's elements is trusted");
                }
                elements
            }
            _ => targets.iter().map(|o| o.field(Step::Elements)).collect(),
        }
    }

    /// The container a `for` walks through `Iterable::iter` or `IterableMut::iter_mut`, the
    /// call the checker writes for `for e in xs` (spec §10.4); `None` for anything else.
    fn iterated_container(&self, subject: ExprId) -> Option<ExprId> {
        let hir::ExprKind::Call { callee: Callee::Method { recv, owner, name, .. }, args } =
            &self.body.expr(subject).kind
        else {
            return None;
        };
        let walks = [("Iterable", "iter"), ("IterableMut", "iter_mut")]
            .iter()
            .any(|(t, m)| name == m && self.table.prelude_trait_named(t) == Some(*owner));
        (walks && args.is_empty()).then_some(*recv)
    }

    /// Where each element of the container `base` denotes is, as a loop over it sees them.
    /// Through `Index` they are what `&xs[i]` points into, slot and owned memory apart;
    /// a type that can only be iterated has its elements named whole at `[*]` (§6.7), so
    /// any kill under the container ends them.
    fn iterated_origins(&mut self, base: ExprId, value: &Value, span: Span) -> Vec<Origin> {
        if index_signature(self.body.ty(base), self.table, self.res).is_some() {
            return self.index_origins(base, value, span);
        }
        self.targets(base, value)
            .iter()
            .map(|o| Origin {
                target: o.target.followed_by(&[Step::Elements]),
                tie: o.target.clone(),
                broad: true,
                payload: None,
                dead: o.dead.clone(),
            })
            .collect()
    }

    fn call(&mut self, callee: &Callee, args: &[ExprId], expression: ExprId) -> Value {
        let held_start = self.held.len();
        let recv = match callee {
            Callee::Method { recv, .. } | Callee::BaseMethod { recv, .. } => {
                let mut value = self.expr(*recv);
                // A method called through a pointer acts on its pointee, never on the
                // place holding the pointer: `c.p.add()` must not kill `c`.
                if matches!(self.body.ty(*recv), Ty::Ptr { .. } | Ty::OwnPtr { .. }) {
                    value.places = value.views.clone();
                }
                Some(value)
            }
            Callee::Value(value) => {
                self.expr(*value);
                None
            }
            _ => None,
        };
        let has_recv = recv.is_some();
        if let Some(recv) = recv {
            self.held.push(recv);
        }
        for arg in args {
            let value = self.expr(*arg);
            self.held.push(value);
        }
        let span = self.body.expr(expression).span;
        if !self.live {
            self.held.truncate(held_start);
            return Value::default();
        }
        let mut held = self.held.split_off(held_start).into_iter();
        let recv = if has_recv { held.next() } else { None };
        let values = held.collect::<Vec<_>>();
        if let Some(value) = &recv {
            if let Callee::Method { recv, .. } | Callee::BaseMethod { recv, .. } = callee {
                self.read(value, self.body.expr(*recv).span);
            }
        }
        for (arg, value) in args.iter().zip(&values) {
            self.read(value, self.body.expr(*arg).span);
        }
        if let Some(signature) =
            call_signature(self.body, callee, args, expression, self.table, self.res)
        {
            // A default left out is the callee's own expression, which cannot name the
            // caller's places: it kills nothing here, and its provenance is unknown, which
            // `invoke` reports only where a clause or tie depends on that parameter.
            let mut result = self.invoke(&signature, recv.as_ref(), &values, span);
            if let Callee::Method { owner, .. } = callee {
                let index = self.table.prelude_trait_named("Index") == Some(*owner)
                    || self.table.prelude_trait_named("IndexMut") == Some(*owner);
                if index {
                    if let Some(recv) = &recv {
                        let parents =
                            if recv.places.is_empty() { &recv.views } else { &recv.places };
                        let mut elements = Vec::new();
                        for parent in parents {
                            for origin in &result.views {
                                if parent.target.covers(&origin.target) {
                                    let mut origin = origin.clone();
                                    origin.target = parent.target.followed_by(&[Step::Elements]);
                                    elements.push(origin);
                                }
                            }
                        }
                        result.views = elements;
                    }
                }
            }
            return result;
        }
        // A call through a function value reads the function type's clause, the most a
        // function stored in it may kill, as a direct call reads its callee's (§6.7). A
        // pointer from C has none, and kills what its `&mut` arguments reach.
        if let Callee::Value(value) = callee {
            if let Ty::Fn { params, ret, kills } = self.body.ty(*value) {
                let signature = Signature {
                    sig: dz_check::ties::fn_type_sig(params, ret, kills),
                    module: self.module(),
                    name: "the function value".into(),
                    self_ty: None,
                    owner: None,
                };
                return self.invoke(&signature, None, &values, span);
            }
        }
        self.incomplete(span, "the callee's signature is unknown");
        let ret = self.body.ty(expression);
        let carries = super::points(ret) || super::contains_views(ret, self.table);
        Value { unknown: carries, ..Value::default() }
    }

    /// The module this body is in, which a function type's clause is read in.
    fn module(&self) -> ModuleId {
        let sym = match &self.body.owner {
            BodyOwner::Fn(s)
            | BodyOwner::Copy(s)
            | BodyOwner::Drop(s)
            | BodyOwner::Const(s)
            | BodyOwner::Static(s) => Some(*s),
            BodyOwner::Method { owner, .. }
            | BodyOwner::Ctor { owner, .. }
            | BodyOwner::AssocConst { owner, .. }
            | BodyOwner::FieldDefault { owner, .. }
            | BodyOwner::TraitDefaults { owner, .. } => Some(*owner),
            BodyOwner::ImplMethod { imp, .. } => {
                return self.table.impls()[*imp as usize].module;
            }
            #[allow(unreachable_patterns)]
            _ => None,
        };
        sym.map_or(ModuleId(0), |s| self.res.symbol(s).module)
    }

    fn invoke(
        &mut self,
        signature: &Signature,
        recv: Option<&Value>,
        args: &[Value],
        span: Span,
    ) -> Value {
        let scope = signature.scope(self.table, self.res);
        let origins = |value: &Value, reference: bool| {
            if reference {
                value.views.clone()
            } else {
                value.places.clone()
            }
        };
        let receiver = recv
            .map(|r| if r.places.is_empty() { r.views.clone() } else { r.places.clone() })
            .unwrap_or_default();
        let params = signature
            .sig
            .params
            .iter()
            .enumerate()
            .map(|(i, p)| args.get(i).map(|v| origins(v, super::points(&p.ty))).unwrap_or_default())
            .collect::<Vec<_>>();
        let first = |origins: &[Origin]| origins.first().map(|o| o.target.clone());
        let initial = Bindings {
            receiver: first(&receiver),
            params: params.iter().map(|o| first(o)).collect(),
        };
        let mut variants = vec![initial.clone()];
        // Every tie has exactly one root, so varying one argument at a time gives the
        // union without a Cartesian product of branch-merged argument origins.
        for origin in receiver.iter().skip(1) {
            variants.push(Bindings { receiver: Some(origin.target.clone()), ..initial.clone() });
        }
        for (i, origins) in params.iter().enumerate() {
            for origin in origins.iter().skip(1) {
                let mut bindings = initial.clone();
                bindings.params[i] = Some(origin.target.clone());
                variants.push(bindings);
            }
        }
        let mut value = Value::default();
        let mut kills = Vec::new();
        for bindings in variants {
            match super::call_effects(&scope, &bindings) {
                Ok(effects) => {
                    if effects.incomplete {
                        self.incomplete(span, "a call effect has unknown provenance");
                    }
                    for kill in effects.kills {
                        if !kills.contains(&kill.place) {
                            kills.push(kill.place);
                        }
                    }
                    match effects.result {
                        ReturnTie::Explicit(place) | ReturnTie::Elided(place) => {
                            let broad = !matches!(place.steps.last(), Some(Step::Storage));
                            let origin = Origin {
                                target: logical_result(signature, &scope, &bindings, &place),
                                tie: place,
                                broad,
                                payload: None,
                                dead: None,
                            };
                            if !value.views.contains(&origin) {
                                value.views.push(origin);
                            }
                            if let Some(root) = result_root(signature, &scope) {
                                let input = match root {
                                    dz_check::ties::Root::SelfValue => &receiver,
                                    dz_check::ties::Root::Param(i) => &params[i],
                                };
                                for origin in input {
                                    if !value.views.contains(origin) {
                                        value.views.push(origin.clone());
                                    }
                                }
                            }
                        }
                        ReturnTie::Unknown => {
                            value.unknown = true;
                            if !receiver.is_empty() || params.iter().any(|p| !p.is_empty()) {
                                self.unchecked(
                                    span,
                                    "a returned view has no unambiguous input tie",
                                );
                            }
                        }
                        ReturnTie::None => {}
                    }
                }
                Err(_) => self.incomplete(span, "a callee's tie path does not resolve"),
            }
        }
        for kill in kills {
            // Independent argument aliases that the call can invalidate cannot safely
            // be given to it. The argument through which this kill happens is allowed:
            // `reset(&mut world)` is what the clause is for.
            let receiver_owns_kill = signature.sig.kills.iter().any(|path| {
                super::normalize(&scope, path).ok().is_some_and(|p| {
                    p.root == dz_check::ties::Root::SelfValue
                        && receiver.iter().any(|o| o.target.followed_by(&p.steps) == kill)
                })
            }) || (signature.sig.kills_unknown
                && signature.sig.takes_self == Some(true)
                && receiver.iter().any(|o| o.target == kill));
            if !receiver_owns_kill {
                for origin in &receiver {
                    if origin.ended_by(&kill, Reach::Owned) && self.reported.insert(span) {
                        self.result.invalid_uses.push(InvalidUse {
                            span,
                            tie: origin.tie.clone(),
                            next_pass: false,
                            killed: BodyKill { place: kill.clone(), span, kind: KillKind::Call },
                        });
                    }
                }
            }
            for (i, origins) in params.iter().enumerate() {
                let owns_kill = signature.sig.kills.iter().any(|path| {
                    super::normalize(&scope, path).ok().is_some_and(|p| {
                        p.root == dz_check::ties::Root::Param(i)
                            && origins.iter().any(|o| o.target.followed_by(&p.steps) == kill)
                    })
                }) || (signature.sig.kills_unknown
                    && super::mutable_input(&signature.sig.params[i].ty)
                    && origins.iter().any(|o| o.target == kill));
                // A value parameter is a copy made before the callee runs, so where it was
                // read from cannot dangle inside the call: `counts.insert(*n, 0)`.
                let holds = super::points(&signature.sig.params[i].ty);
                if !owns_kill && holds {
                    for origin in origins {
                        if origin.ended_by(&kill, Reach::Owned) && self.reported.insert(span) {
                            self.result.invalid_uses.push(InvalidUse {
                                span,
                                tie: origin.tie.clone(),
                                next_pass: false,
                                killed: BodyKill {
                                    place: kill.clone(),
                                    span,
                                    kind: KillKind::Call,
                                },
                            });
                        }
                    }
                }
            }
            self.kill(kill, span, KillKind::Call);
        }
        value
    }

    fn branches(&mut self, blocks: &[Option<BlockId>]) -> Value {
        let entry = self.state.clone();
        let held_entry = self.held.clone();
        let scope_entry = self.scopes.clone();
        let mut joined = None;
        let mut held_joined: Option<Vec<Value>> = None;
        let mut value = Value::default();
        for block in blocks {
            self.state = entry.clone();
            self.held = held_entry.clone();
            self.scopes = scope_entry.clone();
            self.live = true;
            let branch = block.map(|b| self.block(b)).unwrap_or_default();
            if self.live {
                merge_value(&mut value, &branch);
                if let Some(joined) = &mut joined {
                    merge_state(joined, &self.state);
                } else {
                    joined = Some(self.state.clone());
                }
                if let Some(held) = &mut held_joined {
                    merge_held(held, &self.held);
                } else {
                    held_joined = Some(self.held.clone());
                }
            }
        }
        self.live = joined.is_some();
        if self.live {
            self.scopes = scope_entry;
        }
        self.state = joined.unwrap_or(entry);
        self.held = held_joined.unwrap_or(held_entry);
        value
    }

    fn loop_body(
        &mut self,
        cond: Option<ExprId>,
        body: BlockId,
        label: Option<u32>,
        fresh: &[(LocalId, Value)],
        iterator: Option<(Span, Value)>,
    ) {
        let iterator_key = iterator.as_ref().map(|(_, value)| {
            let key = LocalId((self.body.locals.len() + self.loops.len()) as u32);
            self.state.insert(key, value.clone());
            key
        });
        let entry = self.state.clone();
        let held_entry = self.held.clone();
        let scope_entry = self.scopes.clone();
        let mut header = entry.clone();
        let mut held_header = held_entry;
        self.loops.push(Loop { label, scope_depth: self.scopes.len(), ..Loop::default() });
        let frame = self.loops.len() - 1;
        let mut exit = entry;
        let mut held_exit = held_header.clone();
        let mut exit_live = true;
        for iteration in 0..32 {
            self.loops[frame].breaks.clear();
            self.loops[frame].continues.clear();
            self.state = header.clone();
            self.held = held_header.clone();
            self.scopes = scope_entry.clone();
            self.live = true;
            if let Some(cond) = cond {
                self.expr(cond);
            }
            if let (Some(key), Some((span, _))) = (iterator_key, &iterator) {
                if let Some(value) = self.state.get(&key).cloned() {
                    let found = self.result.invalid_uses.len();
                    self.read(&value, *span);
                    for u in &mut self.result.invalid_uses[found..] {
                        u.next_pass = true;
                    }
                }
            }
            let condition_exit = self.state.clone();
            let condition_live = self.live;
            let condition_held = self.held.clone();
            for (local, value) in fresh {
                self.bind(*local, value.clone());
            }
            if self.live {
                self.block(body);
            }
            let mut next = header.clone();
            let mut held_next = held_header.clone();
            if self.live {
                merge_state(&mut next, &self.state);
                merge_held(&mut held_next, &self.held);
            }
            for (state, held) in &self.loops[frame].continues {
                merge_state(&mut next, state);
                merge_held(&mut held_next, held);
            }
            exit = condition_exit;
            held_exit = condition_held;
            exit_live = condition_live || !self.loops[frame].breaks.is_empty();
            for (state, held) in &self.loops[frame].breaks {
                merge_state(&mut exit, state);
                merge_held(&mut held_exit, held);
            }
            let mut bounded = false;
            for value in next.values_mut().chain(&mut held_next) {
                for origins in [&mut value.places, &mut value.views] {
                    if origins.len() > 64
                        || origins
                            .iter()
                            .any(|o| o.target.steps.len() > 64 || o.tie.steps.len() > 64)
                    {
                        origins.truncate(64);
                        origins.retain(|o| o.target.steps.len() <= 64 && o.tie.steps.len() <= 64);
                        bounded = true;
                        value.unknown = true;
                    }
                }
            }
            if bounded {
                self.incomplete(
                    self.body.block(body).span,
                    "loop provenance exceeded the analysis bound",
                );
            }
            if next == header && held_next == held_header {
                break;
            }
            header = next;
            held_header = held_next;
            if iteration == 31 {
                self.incomplete(self.body.block(body).span, "loop provenance did not converge");
            }
        }
        self.loops.pop();
        self.state = exit;
        if let Some(key) = iterator_key {
            self.state.remove(&key);
        }
        self.held = held_exit;
        if exit_live {
            self.scopes = scope_entry;
        }
        self.live = exit_live;
    }

    /// A reference to the payload of the union or optional `subject` denotes.
    fn payload_ref(&self, subject: ExprId, value: &Value) -> Value {
        let views = self.targets(subject, value).iter().map(Origin::payload).collect();
        Value { views, unknown: value.unknown, ..Value::default() }
    }

    /// The payload of an optional, taken out by value. An optional does not carry the
    /// views of what it holds yet, so a view taken out of one has unknown provenance.
    fn unwrapped(&mut self, operand: ExprId, value: Value, span: Span) -> Value {
        if let Ty::Optional(inner) = self.body.ty(operand).peeled_refs() {
            if super::contains_views(inner, self.table) && value.views.is_empty() {
                self.unchecked(span, "optional view provenance is not modeled yet");
                return Value { unknown: true, ..Value::default() };
            }
        }
        value
    }

    fn unchecked(&mut self, span: Span, reason: &'static str) {
        let item = Incomplete { span, reason };
        if !self.result.unchecked.contains(&item) {
            self.result.unchecked.push(item);
        }
    }

    fn incomplete(&mut self, span: Span, reason: &'static str) {
        let item = Incomplete { span, reason };
        if !self.result.incomplete.contains(&item) {
            self.result.incomplete.push(item);
        }
    }

    fn read(&mut self, value: &Value, span: Span) {
        if self.reported.contains(&span) {
            return;
        }
        if let Some(origin) = value.views.iter().chain(&value.places).find(|o| o.dead.is_some()) {
            // `observe(&c.r)` reads `c` first; the argument around it is the same mistake.
            let killed = origin.dead.as_ref().unwrap();
            if self.result.invalid_uses.iter().any(|u| {
                &u.killed == killed && u.span.start >= span.start && u.span.end <= span.end
            }) {
                return;
            }
            self.result.invalid_uses.push(InvalidUse {
                span,
                tie: origin.tie.clone(),
                next_pass: false,
                killed: origin.dead.clone().unwrap(),
            });
            self.reported.insert(span);
        }
    }

    fn kill(&mut self, place: Place, span: Span, kind: KillKind) {
        let killed = BodyKill { place, span, kind };
        for value in self.state.values_mut().chain(&mut self.held) {
            for origin in value.views.iter_mut().chain(&mut value.places) {
                if origin.dead.is_none() && origin.ended_by(&killed.place, killed.kind.reach()) {
                    origin.dead = Some(killed.clone());
                }
            }
        }
        if !self.result.kills.contains(&killed) {
            self.result.kills.push(killed);
        }
    }

    fn bind(&mut self, local: LocalId, value: Value) {
        if super::points(&self.body.local(local).ty) {
            self.state.insert(local, Value { places: Vec::new(), ..value });
        } else {
            if super::contains_views(&self.body.local(local).ty, self.table) {
                self.unchecked(
                    self.body.local(local).span,
                    "aggregate view bindings are not modeled yet",
                );
            }
            self.state.remove(&local);
        }
    }

    fn block(&mut self, block: BlockId) -> Value {
        self.scopes.push((Vec::new(), Vec::new()));
        let mut value = Value::default();
        for stmt in self.body.block(block).stmts.clone() {
            if !self.live {
                break;
            }
            value = self.stmt(stmt);
        }
        let (locals, deferred) = self.scopes.pop().unwrap();
        // Drops can end a view returned by the block. This includes a local declared
        // inside a nested block and referenced by a binding outside it.
        for local in locals.into_iter().rev() {
            self.kill(Place::local(local), self.body.block(block).span, KillKind::Drop);
            self.state.remove(&local);
            for origin in value.views.iter_mut().chain(&mut value.places) {
                if origin.dead.is_none() && origin.ended_by(&Place::local(local), Reach::All) {
                    origin.dead = Some(BodyKill {
                        place: Place::local(local),
                        span: self.body.block(block).span,
                        kind: KillKind::Drop,
                    });
                }
            }
        }
        if !deferred.is_empty() {
            // A diverging edge still executes its defers; replay with liveness restored
            // only for these statements, after the scope's drops (§6.5).
            let live = self.live;
            self.live = true;
            for stmt in deferred.into_iter().rev() {
                self.stmt(stmt);
            }
            self.live = live;
        }
        value
    }

    fn stmt(&mut self, id: StmtId) -> Value {
        let node = self.body.stmt(id).clone();
        match node.kind {
            hir::StmtKind::Let { local, init } => {
                let value = init.map(|e| self.expr(e)).unwrap_or_default();
                if let Some(local) = local {
                    self.bind(local, value);
                    self.scopes.last_mut().unwrap().0.push(local);
                }
            }
            hir::StmtKind::LetTuple { locals, init } => {
                self.expr(init);
                self.unchecked(node.span, "tuple view destructuring is not modeled yet");
                for local in locals.into_iter().flatten() {
                    self.bind(local, Value { unknown: true, ..Value::default() });
                    self.scopes.last_mut().unwrap().0.push(local);
                }
            }
            hir::StmtKind::Assign { target, op, value } => {
                // The emitter evaluates the RHS before the destination.
                let value = self.expr(value);
                let ty = self.body.ty(target);
                if op == hir::AssignOp::Assign {
                    if let hir::ExprKind::Local(local) = self.body.expr(target).kind {
                        if super::points(ty) && !matches!(ty, Ty::Ref { .. }) {
                            // Pointer/slice/str assignment retargets the local; assigning
                            // to a reference writes the referent instead.
                            self.bind(local, value);
                            return Value::default();
                        }
                    }
                }
                let destination = self.expr(target);
                if op == hir::AssignOp::Assign {
                    if matches!(self.body.expr(target).kind, hir::ExprKind::Field { .. })
                        && matches!(ty, Ty::Ptr { .. } | Ty::OwnPtr { .. })
                    {
                        // A tied pointer field now names other memory. Only a tied one
                        // has views; an untied one stays unchecked (§6.7).
                        for origin in &destination.views {
                            self.kill(origin.target.clone(), node.span, KillKind::Assign);
                        }
                    } else if moves_memory(ty.peeled_refs(), self.table) {
                        // The slot itself stays where it is: a view of it, or of another
                        // element, survives, and only what the old value owned dies.
                        for place in &destination.places {
                            self.kill(place.target.clone(), node.span, KillKind::Assign);
                        }
                        if destination.places.is_empty() {
                            self.incomplete(node.span, "assignment through unknown storage");
                        }
                    }
                }
            }
            hir::StmtKind::Expr(e) => return self.expr(e),
            hir::StmtKind::While { cond, body, label } => {
                self.loop_body(Some(cond), body, label.map(|l| l.id), &[], None);
            }
            hir::StmtKind::For { bindings, subject, body, label, shape } => {
                let span = self.body.stmt(id).span;
                // A container is walked through the `iter()` its loop calls, whose result
                // is a struct holding a view. Its elements are the container's `[*]`
                // (§6.7), so they and the iterator are followed from the container itself.
                let walked = self.iterated_container(subject);
                let (element, iterator) = match walked {
                    Some(container) => {
                        let value = self.expr(container);
                        let element = self.iterated_origins(container, &value, span);
                        let iterator = Value { views: element.clone(), ..Value::default() };
                        (element, iterator)
                    }
                    None => {
                        let value = self.expr(subject);
                        let element = self.index_origins(subject, &value, span);
                        let iterator = Value {
                            views: self.targets(subject, &value),
                            unknown: value.unknown,
                            ..Value::default()
                        };
                        (element, iterator)
                    }
                };
                let fresh = bindings
                    .into_iter()
                    .flatten()
                    .map(|local| {
                        let value = if super::points(&self.body.local(local).ty) {
                            Value { views: element.clone(), ..Value::default() }
                        } else {
                            Value::default()
                        };
                        (local, value)
                    })
                    .collect::<Vec<_>>();
                // The parts of a container's element all point into it; an iterator's
                // tuple of views could point anywhere.
                if matches!(shape, hir::ForShape::Tuple) && walked.is_none() {
                    self.unchecked(node.span, "tuple iterator bindings are not modeled yet");
                }
                self.loop_body(
                    None,
                    body,
                    label.map(|l| l.id),
                    &fresh,
                    Some((self.body.expr(subject).span, iterator)),
                );
            }
            hir::StmtKind::Defer(stmt) => self.scopes.last_mut().unwrap().1.push(stmt),
            hir::StmtKind::Item(_) | hir::StmtKind::Error => {}
        }
        Value::default()
    }

    fn kill_places(&mut self, places: &[Origin], span: Span, kind: KillKind) {
        for place in places {
            self.kill(place.target.clone(), span, kind);
            if place.tie != place.target && place.tie.local == place.target.local {
                // Replacing a slot ends its inline references as well as what the
                // old element owned. Reallocating the array alone still ends only
                // the address tie; the two effects must not be conflated.
                self.kill(place.tie.clone(), span, kind);
            }
        }
    }

    fn unwind(&mut self, depth: usize) {
        // Capture jumps after exited scopes drop. Taking the work prevents defers
        // from running twice as the enclosing structured blocks return.
        for index in (depth..self.scopes.len()).rev() {
            let (locals, deferred) = std::mem::take(&mut self.scopes[index]);
            for local in locals.into_iter().rev() {
                self.kill(Place::local(local), self.body.local(local).span, KillKind::Drop);
                self.state.remove(&local);
            }
            for stmt in deferred.into_iter().rev() {
                self.stmt(stmt);
            }
        }
    }

    fn expr(&mut self, id: ExprId) -> Value {
        if !self.live {
            return Value::default();
        }
        let node = self.body.expr(id).clone();
        let mut value = match node.kind {
            hir::ExprKind::Local(local) => {
                let mut value = self.state.get(&local).cloned().unwrap_or_default();
                if matches!(node.ty, Ty::Ref { .. }) || self.body.local(local).name == "self" {
                    value.places = value.views.clone();
                } else {
                    value.places = vec![Origin::at(Place::local(local))];
                }
                self.read(&value, node.span);
                value
            }
            hir::ExprKind::Ref { place, .. } => {
                let value = self.expr(place);
                Value { views: value.places, unknown: value.unknown, ..Value::default() }
            }
            hir::ExprKind::Deref(inner) => {
                let value = self.expr(inner);
                Value { places: value.views, unknown: value.unknown, ..Value::default() }
            }
            hir::ExprKind::Field { base, index } => self.field(base, index),
            hir::ExprKind::TupleIndex { base, .. } => {
                self.expr(base);
                self.unchecked(node.span, "tuple field provenance is not modeled yet");
                Value { unknown: super::points(&node.ty), ..Value::default() }
            }
            hir::ExprKind::Index { base, index } => {
                let value = self.expr(base);
                self.expr(index);
                Value {
                    places: self.index_origins(base, &value, node.span),
                    unknown: value.unknown,
                    ..Value::default()
                }
            }
            hir::ExprKind::SliceRange { base, start, end } => {
                let value = self.expr(base);
                for end in [start, end].into_iter().flatten() {
                    self.expr(end);
                }
                let views = self.targets(base, &value);
                Value { views, unknown: value.unknown, ..Value::default() }
            }
            hir::ExprKind::DataPtr(base) => {
                let value = self.expr(base);
                Value {
                    views: self.targets(base, &value),
                    unknown: value.unknown,
                    ..Value::default()
                }
            }
            hir::ExprKind::Len(base) => {
                self.expr(base);
                Value::default()
            }
            hir::ExprKind::Move(inner) => {
                let mut value = self.expr(inner);
                if let hir::ExprKind::Local(local) = self.body.expr(inner).kind {
                    self.kill(Place::local(local), node.span, KillKind::Move);
                } else {
                    self.kill_places(&value.places, node.span, KillKind::Move);
                    if value.places.is_empty() {
                        self.incomplete(node.span, "a move out of unknown storage");
                    }
                }
                // The moved value's owned storage gets a new home. Views into its
                // source are dead; preserving move-destination provenance is a later
                // placement concern. Moving a reference still copies its provenance.
                value.places.clear();
                value
            }
            hir::ExprKind::Call { callee, args } => self.call(&callee, &args, id),
            hir::ExprKind::Block(block) => self.block(block),
            hir::ExprKind::If { cond, then, else_branch } => {
                self.expr(cond);
                if !self.live {
                    return Value::default();
                }
                self.branches(&[Some(then), else_branch])
            }
            hir::ExprKind::IfLet { binding, operand, then, else_branch } => {
                let value = self.expr(operand);
                if !self.live {
                    return Value::default();
                }
                // `if let &v = opt` refers to the payload where it lives (§9); `if let v`
                // copies it out, and `if let &v = ptr` is the pointee, as the value is.
                let bound = match self.body.ty(operand).peeled_refs() {
                    Ty::Optional(inner)
                        if matches!(self.body.local(binding).ty, Ty::Ref { .. })
                            && self.body.local(binding).ty != **inner =>
                    {
                        self.payload_ref(operand, &value)
                    }
                    _ => self.unwrapped(operand, value, node.span),
                };
                self.bind(binding, bound);
                self.branches(&[Some(then), else_branch])
            }
            hir::ExprKind::Switch { subject, arms, default } => {
                let value = self.expr(subject);
                if !self.live {
                    return Value::default();
                }
                let mut blocks = Vec::new();
                for arm in arms {
                    if let Some(binding) = arm.binding {
                        // `case Circle(&c)` refers to the payload; `case Circle(c)` is a
                        // copy, whose own views `bind` cannot follow yet.
                        let bound = if matches!(self.body.local(binding).ty, Ty::Ref { .. }) {
                            self.payload_ref(subject, &value)
                        } else {
                            Value { unknown: value.unknown, ..Value::default() }
                        };
                        self.bind(binding, bound);
                    }
                    blocks.push(Some(arm.body));
                }
                if let Some(default) = default {
                    blocks.push(Some(default));
                }
                self.branches(&blocks)
            }
            hir::ExprKind::Or { operand, clause } => {
                let value = self.expr(operand);
                if !self.live {
                    return Value::default();
                }
                let mut value = self.unwrapped(operand, value, node.span);
                let entry = self.state.clone();
                let held_entry = self.held.clone();
                let scope_entry = self.scopes.clone();
                let fallback = match clause {
                    hir::OrClause::Value(e) => self.expr(e),
                    hir::OrClause::Block { binding, body } => {
                        if let Some(binding) = binding {
                            self.bind(binding, Value::default());
                        }
                        self.block(body)
                    }
                    hir::OrClause::Ignore | hir::OrClause::Unreachable => Value::default(),
                };
                if self.live {
                    merge_value(&mut value, &fallback);
                    merge_state(&mut self.state, &entry);
                    merge_held(&mut self.held, &held_entry);
                } else {
                    self.state = entry;
                    self.held = held_entry;
                    self.scopes = scope_entry;
                    self.live = true;
                }
                value
            }
            hir::ExprKind::Return(value) => {
                let held_start = self.held.len();
                if let Some(value) = value {
                    let value = self.expr(value);
                    // Scope-exit kills are applied below; returning a view of a local
                    // must be diagnosed before that local disappears from the state.
                    for origin in &value.views {
                        let caller_owned = self.body.params.contains(&origin.target.local)
                            && (super::points(&self.body.local(origin.target.local).ty)
                                || self.body.local(origin.target.local).name == "self");
                        if !caller_owned {
                            let killed = BodyKill {
                                place: Place::local(origin.target.local),
                                span: node.span,
                                kind: KillKind::Drop,
                            };
                            if self.reported.insert(node.span) {
                                self.result.invalid_uses.push(InvalidUse {
                                    span: node.span,
                                    tie: origin.tie.clone(),
                                    next_pass: false,
                                    killed,
                                });
                            }
                        }
                    }
                    if !self.reported.contains(&node.span) {
                        match &self.expected_return {
                            ReturnTie::Explicit(expected) | ReturnTie::Elided(expected) => {
                                // Spelt as the ties that name them, as the signature's
                                // `in` is: `self.m_books[*]` is `self[*]` (§6.7).
                                let views: Vec<Origin> =
                                    value.views.iter().map(|o| self.canonical(o.clone())).collect();
                                if let Some(origin) = views.iter().find(|o| {
                                    !views.iter().any(|other| {
                                        other.target == o.target
                                            && (expected.covers(&other.tie)
                                                || expected.covers(&other.target))
                                    })
                                }) {
                                    self.result.invalid_returns.push(InvalidReturn {
                                        span: node.span,
                                        expected: expected.clone(),
                                        actual: origin.tie.clone(),
                                    });
                                    self.reported.insert(node.span);
                                }
                                if value.unknown {
                                    self.unchecked(
                                        node.span,
                                        "the returned view's provenance is unknown",
                                    );
                                }
                            }
                            ReturnTie::Unknown if !value.views.is_empty() => self.unchecked(
                                node.span,
                                "the returned view has no unambiguous declared tie",
                            ),
                            _ => {}
                        }
                    }
                    // A result returned by value moves out before the locals drop; only
                    // its views must survive the drops and defers that follow.
                    self.held.push(Value { places: Vec::new(), ..value });
                }
                self.unwind(0);
                for value in self.held.split_off(held_start) {
                    self.read(&value, node.span);
                }
                self.live = false;
                Value::default()
            }
            hir::ExprKind::Break(label) | hir::ExprKind::Continue(label) => {
                let target = label
                    .and_then(|label| self.loops.iter().rposition(|l| l.label == Some(label)))
                    .or_else(|| self.loops.len().checked_sub(1));
                if let Some(target) = target {
                    let depth = self.loops[target].scope_depth;
                    self.unwind(depth);
                    let state = self.state.clone();
                    if matches!(node.kind, hir::ExprKind::Break(_)) {
                        self.loops[target].breaks.push((state, self.held.clone()));
                    } else {
                        self.loops[target].continues.push((state, self.held.clone()));
                    }
                }
                self.live = false;
                Value::default()
            }
            hir::ExprKind::Guarded { guard, init, value } => {
                let init = self.expr(init);
                self.bind(guard, init);
                self.expr(value)
            }
            hir::ExprKind::Formatted { args, params, body } => {
                for (arg, param) in args.into_iter().zip(params) {
                    let value = self.expr(arg);
                    self.bind(param, value);
                }
                self.block(body)
            }
            hir::ExprKind::Unary { operand, .. } => {
                self.expr(operand);
                Value::default()
            }
            hir::ExprKind::Cast { operand, explicit } => {
                let value = self.expr(operand);
                // The checker dropping `mut`, or a `str` read as its bytes, is the same
                // memory. A pointer made by a cast is unchecked, as §6.7 says: written
                // `as`, it is `unsafe`, and it is how a body keeps the old allocation
                // while it moves the elements out of it.
                let same = pointee(&node.ty).is_some_and(|to| {
                    pointee(self.body.ty(operand)).is_some_and(|from| from == to)
                });
                let made = explicit && matches!(node.ty, Ty::Ptr { .. } | Ty::OwnPtr { .. });
                if super::points(&node.ty) && (!same || made) {
                    Value { unknown: true, ..Value::default() }
                } else {
                    value
                }
            }
            hir::ExprKind::Binary { op, lhs, rhs } => {
                self.expr(lhs);
                if !self.live {
                    return Value::default();
                }
                if matches!(op, hir::BinaryOp::And | hir::BinaryOp::Or) {
                    let entry = self.state.clone();
                    let held_entry = self.held.clone();
                    let scope_entry = self.scopes.clone();
                    self.expr(rhs);
                    if self.live {
                        merge_state(&mut self.state, &entry);
                        merge_held(&mut self.held, &held_entry);
                    } else {
                        self.state = entry;
                        self.held = held_entry;
                        self.scopes = scope_entry;
                        self.live = true;
                    }
                } else {
                    self.expr(rhs);
                }
                Value::default()
            }
            hir::ExprKind::Range { start, end, .. } => {
                for e in [start, end].into_iter().flatten() {
                    self.expr(e);
                }
                Value::default()
            }
            hir::ExprKind::StructLit { fields, .. }
            | hir::ExprKind::UnionVariant { payload: fields, .. } => {
                for e in fields.into_iter().flatten() {
                    self.expr(e);
                }
                Value::default()
            }
            hir::ExprKind::Tuple(parts) | hir::ExprKind::ArrayLit(parts) => {
                for e in parts {
                    self.expr(e);
                }
                if super::contains_views(&node.ty, self.table) {
                    self.unchecked(node.span, "aggregate view provenance is not modeled yet");
                }
                Value::default()
            }
            hir::ExprKind::Builtin { name, args } | hir::ExprKind::Intrinsic { name, args } => {
                for (i, arg) in args.into_iter().enumerate() {
                    let value = self.expr(arg);
                    if i == 0 && matches!(name.as_str(), "destroy" | "move_out") {
                        self.kill_places(&value.places, node.span, KillKind::Drop);
                        if value.places.is_empty() {
                            self.incomplete(node.span, "destruction through unknown storage");
                        }
                    }
                }
                Value { unknown: super::points(&node.ty), ..Value::default() }
            }
            // A static is reached only in `unsafe`, and no clause can name one, so it is
            // unchecked, like a pointer made there (§6.7): no place a kill could reach.
            hir::ExprKind::Static(_) => Value {
                unknown: super::points(&node.ty) || super::contains_views(&node.ty, self.table),
                ..Value::default()
            },
            hir::ExprKind::DefaultArg(_) => {
                Value { unknown: super::points(&node.ty), ..Value::default() }
            }
            _ => Value::default(),
        };
        if matches!(node.ty, Ty::Never) {
            self.live = false;
        }
        // A place expression of reference type denotes the pointee as well.
        if matches!(node.ty, Ty::Ref { .. }) && value.places.is_empty() {
            value.places = value.views.clone();
        }
        value
    }
}

/// Whether writing over a value of `ty` can end a view: the old value owns memory (it has
/// a `drop`, or a tied pointer), or it holds a union's or an optional's payload, whose
/// variant may change. A plain value is overwritten in place (spec §6.7). An untied
/// pointer is unchecked, so it counts only through its owner's `drop`.
pub(super) fn moves_memory(ty: &Ty, table: &TypeTable) -> bool {
    fn go(ty: &Ty, table: &TypeTable, depth: usize) -> bool {
        if depth > 8 {
            return true;
        }
        match ty {
            Ty::Optional(_) | Ty::Param(_) | Ty::Assoc(_) => true,
            Ty::Array { elem, .. } => go(elem, table, depth + 1),
            Ty::Tuple(parts) => parts.iter().any(|p| go(p, table, depth + 1)),
            Ty::Named { sym, args } => match table.get(*sym) {
                Some(ItemSig::Struct(s)) => {
                    s.drop.is_some()
                        || s.fields.iter().zip(table.field_types(*sym, args, &s.fields)).any(
                            |(f, t)| {
                                (f.tie.is_some() && matches!(t, Ty::Ptr { .. } | Ty::OwnPtr { .. }))
                                    || go(&t, table, depth + 1)
                            },
                        )
                }
                Some(ItemSig::Union { variants, .. }) => {
                    variants.iter().any(|v| !table.payload_types(*sym, args, v).is_empty())
                }
                _ => false,
            },
            _ => false,
        }
    }
    go(ty, table, 0)
}

fn pointee(ty: &Ty) -> Option<&Ty> {
    const BYTE: Ty = Ty::Int(ty::IntTy::U8);
    match ty {
        Ty::Ref { inner, .. } | Ty::Ptr { inner, .. } | Ty::OwnPtr { inner, .. } => Some(inner),
        Ty::Slice { elem, .. } => Some(elem),
        Ty::Str | Ty::CStr => Some(&BYTE),
        _ => None,
    }
}

fn merge_value(to: &mut Value, from: &Value) {
    for (to, from) in [(&mut to.places, &from.places), (&mut to.views, &from.views)] {
        for origin in from {
            if !to.contains(origin) {
                to.push(origin.clone());
            }
        }
    }
    to.unknown |= from.unknown;
}

fn merge_held(to: &mut [Value], from: &[Value]) {
    for (to, from) in to.iter_mut().zip(from) {
        merge_value(to, from);
    }
}

fn merge_state(to: &mut State, from: &State) {
    for (local, value) in from {
        merge_value(to.entry(*local).or_default(), value);
    }
}

pub(super) struct Signature {
    pub(super) sig: FnSig,
    module: ModuleId,
    pub(super) name: String,
    self_ty: Option<Ty>,
    owner: Option<SymbolId>,
}

impl Signature {
    pub(super) fn scope<'a>(
        &'a self,
        table: &'a TypeTable,
        res: &'a Resolution,
    ) -> dz_check::ties::Scope<'a> {
        dz_check::ties::Scope {
            table,
            res,
            module: self.module,
            name: &self.name,
            sig: &self.sig,
            self_ty: self.self_ty.clone(),
            privileged: self.owner,
        }
    }
}

fn body_bindings(body: &Body) -> Bindings {
    let offset = usize::from(body.params.first().is_some_and(|l| body.local(*l).name == "self"));
    Bindings {
        receiver: if offset == 1 { body.params.first().map(|l| Place::local(*l)) } else { None },
        params: body.params.iter().skip(offset).map(|l| Some(Place::local(*l))).collect(),
    }
}

pub(super) fn body_signature(
    body: &Body,
    table: &TypeTable,
    res: &Resolution,
) -> Option<Signature> {
    let (sig, home, name) = match &body.owner {
        BodyOwner::Fn(sym) => {
            let ItemSig::Fn(sig) = table.get(*sym)? else { return None };
            (sig, *sym, res.symbol(*sym).name.clone())
        }
        BodyOwner::Method { owner, name } | BodyOwner::TraitDefaults { owner, name } => {
            (table.methods_sig(*owner)?.methods.get(name)?, *owner, name.clone())
        }
        BodyOwner::Ctor { owner, index } => {
            let mut sig = table.struct_sig(*owner)?.ctors.get(*index)?.clone();
            sig.takes_self = Some(true);
            sig.ret = Ty::Void;
            return Some(Signature {
                sig,
                module: res.symbol(*owner).module,
                name: "constructor".into(),
                self_ty: body.params.first().map(|l| body.local(*l).ty.clone()),
                owner: Some(*owner),
            });
        }
        BodyOwner::Copy(owner) => (table.struct_sig(*owner)?.copy.as_ref()?, *owner, "copy".into()),
        BodyOwner::Drop(owner) => (table.struct_sig(*owner)?.drop.as_ref()?, *owner, "drop".into()),
        BodyOwner::ImplMethod { imp, name } => {
            let imp = &table.impls()[*imp as usize];
            return Some(Signature {
                sig: imp.methods.get(name)?.clone(),
                module: imp.module,
                name: name.clone(),
                self_ty: Some(imp.self_ty.clone()),
                owner: match imp.self_ty.peeled() {
                    Ty::Named { sym, .. } => Some(*sym),
                    _ => None,
                },
            });
        }
        _ => return None,
    };
    let self_local = body.params.first().map(|l| body.local(*l)).filter(|l| l.name == "self");
    Some(Signature {
        sig: sig.clone(),
        module: res.symbol(home).module,
        name,
        self_ty: self_local.map(|l| l.ty.peeled().clone()),
        owner: self_local.map(|_| home),
    })
}

fn call_signature(
    body: &Body,
    callee: &Callee,
    args: &[ExprId],
    expression: ExprId,
    table: &TypeTable,
    res: &Resolution,
) -> Option<Signature> {
    let (mut sig, home, self_ty, name, type_args, method_args) = match callee {
        Callee::Fn { sym, type_args } => {
            let ItemSig::Fn(sig) = table.get(*sym)? else { return None };
            (sig.clone(), *sym, None, res.symbol(*sym).name.clone(), Vec::new(), type_args.clone())
        }
        Callee::Method { recv, owner, name, method_args } => {
            let recv_ty = body.ty(*recv).peeled();
            if matches!(table.get(*owner), Some(ItemSig::Trait(_))) {
                let arg = args.first().map(|e| body.ty(*e));
                if let Some((index, map)) =
                    table.resolve_impl(*owner, recv_ty, name, arg, Some(body.ty(expression)))
                {
                    let imp = &table.impls()[index];
                    if let Some(sig) = imp.methods.get(name) {
                        return Some(Signature {
                            sig: substitute_signature(sig, &map, Some(recv_ty)),
                            module: imp.module,
                            name: name.clone(),
                            self_ty: Some(recv_ty.clone()),
                            owner: match recv_ty {
                                Ty::Named { sym, .. } => Some(*sym),
                                _ => None,
                            },
                        });
                    }
                }
            }
            let sig = table.methods_sig(*owner)?.methods.get(name)?.clone();
            let type_args = match recv_ty {
                Ty::Named { sym, args } => table.args_as(*sym, args, *owner),
                _ => Vec::new(),
            };
            (sig, *owner, Some(recv_ty.clone()), name.clone(), type_args, method_args.clone())
        }
        Callee::BaseMethod { recv, owner, name } => {
            let recv_ty = body.ty(*recv).peeled();
            let type_args = match recv_ty {
                Ty::Named { sym, args } => table.args_as(*sym, args, *owner),
                _ => Vec::new(),
            };
            (
                table.methods_sig(*owner)?.methods.get(name)?.clone(),
                *owner,
                Some(recv_ty.clone()),
                name.clone(),
                type_args,
                Vec::new(),
            )
        }
        Callee::Assoc { owner, type_args, name, method_args } => (
            table.methods_sig(*owner)?.methods.get(name)?.clone(),
            *owner,
            None,
            name.clone(),
            type_args.clone(),
            method_args.clone(),
        ),
        Callee::Ctor { owner, index } => (
            table.struct_sig(*owner)?.ctors.get(*index)?.clone(),
            *owner,
            None,
            "constructor".into(),
            Vec::new(),
            Vec::new(),
        ),
        Callee::Value(value) => {
            if let hir::ExprKind::FnItem { sym, type_args } = &body.expr(*value).kind {
                return call_signature(
                    body,
                    &Callee::Fn { sym: *sym, type_args: type_args.clone() },
                    args,
                    expression,
                    table,
                    res,
                );
            }
            return None;
        }
    };
    let mut map = table
        .type_params(home)
        .iter()
        .map(|p| p.param.id)
        .zip(type_args)
        .collect::<HashMap<_, _>>();
    map.extend(sig.generics.iter().map(|p| p.param.id).zip(method_args));
    sig = substitute_signature(&sig, &map, self_ty.as_ref());
    Some(Signature { sig, module: res.symbol(home).module, name, self_ty, owner: Some(home) })
}

fn substitute_signature(sig: &FnSig, map: &HashMap<u32, Ty>, self_ty: Option<&Ty>) -> FnSig {
    let mut sig = sig.clone();
    sig.ret = ty::substitute_with(&sig.ret, map, self_ty);
    for param in &mut sig.params {
        param.ty = ty::substitute_with(&param.ty, map, self_ty);
    }
    sig
}

/// A whole-input result retains the input reference's address dependency. Otherwise
/// `identity(&xs[i])` would lose xs@items when its return tie is elided.
fn result_root(
    signature: &Signature,
    scope: &dz_check::ties::Scope<'_>,
) -> Option<dz_check::ties::Root> {
    if let Some(path) = &signature.sig.ret_tie {
        let path = super::normalize(scope, path).ok()?;
        return path.steps.is_empty().then_some(path.root);
    }
    let mut roots = Vec::new();
    if signature.sig.takes_self.is_some() {
        roots.push(dz_check::ties::Root::SelfValue);
    }
    // The same inputs `call_effects` elides over.
    for (i, param) in signature.sig.params.iter().enumerate() {
        if super::points(&param.ty) {
            roots.push(dz_check::ties::Root::Param(i));
        }
    }
    match roots.as_slice() {
        [root] => Some(*root),
        _ => None,
    }
}

/// Returned pointers into an indexed container get the same logical [*] target as
/// indexing, while retaining the allocation tie for address validity. This also works
/// through wrappers such as World::first.
fn logical_result(
    signature: &Signature,
    scope: &dz_check::ties::Scope<'_>,
    bindings: &Bindings,
    place: &Place,
) -> Place {
    let Some(path) = &signature.sig.ret_tie else { return place.clone() };
    let Ok(path) = super::normalize(scope, path) else { return place.clone() };
    let Some(mut target) = bindings.get(path.root).cloned() else { return place.clone() };
    let mut ty = match path.root {
        dz_check::ties::Root::SelfValue => signature.self_ty.clone().unwrap_or(Ty::Error),
        dz_check::ties::Root::Param(i) => signature.sig.params[i].ty.clone(),
    };
    let mut storage_owner = None;
    for step in path.steps {
        match &step {
            Step::Field(name) => {
                let owner = ty.clone();
                let Some(next) = super::field(scope, &ty, name) else { return place.clone() };
                storage_owner = Some((owner, target.clone()));
                ty = next;
            }
            Step::Elements => {
                ty = dz_check::ties::elements(scope, &ty).flatten().unwrap_or(Ty::Error);
                storage_owner = None;
            }
            Step::Storage => {
                if let Some((owner, parent)) = storage_owner.take() {
                    if dz_check::ties::elements(scope, &owner)
                        .flatten()
                        .is_some_and(|t| t.peeled() == signature.sig.ret.peeled())
                    {
                        return parent.followed_by(&[Step::Elements]);
                    }
                }
            }
        }
        target = target.followed_by(&[step]);
    }
    place.clone()
}

fn index_signature(recv: &Ty, table: &TypeTable, res: &Resolution) -> Option<Signature> {
    let trait_sym = table.prelude_trait_named("Index")?;
    let recv = recv.peeled();
    if let Some((index, map)) =
        table.resolve_impl(trait_sym, recv, "index", Some(&Ty::Int(ty::IntTy::Usize)), None)
    {
        let imp = &table.impls()[index];
        if let Some(sig) = imp.methods.get("index") {
            return Some(Signature {
                sig: substitute_signature(sig, &map, Some(recv)),
                module: imp.module,
                name: "index".into(),
                self_ty: Some(recv.clone()),
                owner: match recv {
                    Ty::Named { sym, .. } => Some(*sym),
                    _ => None,
                },
            });
        }
    }
    let sig = table.struct_sig(trait_sym)?.methods.get("index")?.clone();
    Some(Signature {
        sig,
        module: res.symbol(trait_sym).module,
        name: "index".into(),
        self_ty: Some(recv.clone()),
        owner: Some(trait_sym),
    })
}
