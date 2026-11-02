//! Structured, per-function tie analysis (spec §6.7).
//!
//! A reference remembers both its logical target and the tie keeping its address valid.
//! For `&xs[i]` these differ: the target is `xs[*]`, but its address is in `xs@items`.
//! A later view of the element's owned allocation follows the target, not that address.

use std::collections::{HashMap, HashSet};

use dz_ast::TiePath;
use dz_check::{FnSig, ItemSig, TypeTable};
use dz_hir::ty::{self, Ty};
use dz_hir::{self as hir, BlockId, Body, BodyOwner, Callee, ExprId, LocalId, StmtId};
use dz_project::ModuleId;
use dz_resolve::{Resolution, SymbolId};
use dz_span::Span;

use crate::placement::Staying;

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
    /// The implicit `drop` of a value that holds views, at the end of its scope or when it
    /// is assigned over: its `drop` may read them (spec §6.7, T-0127).
    pub by_drop: bool,
}

/// A call that may kill memory through one argument while another reaches it (spec §6.7,
/// D-0094 point 1): the callee was checked with its parameters taken as separate, so inside
/// it the other one dangles. `append_copy(&mut ys, &ys)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AliasedKill {
    /// The call.
    pub span: Span,
    /// What the call kills, in this body.
    pub killed: Place,
    /// The parameter the clause names it by: `self` for the receiver.
    pub through: String,
    /// The other parameter, which reaches it too.
    pub other: String,
    /// The other argument in this body; the receiver has none of its own.
    pub other_span: Option<Span>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvalidReturn {
    pub span: Span,
    /// Every place the signature's `in` names; one when elided.
    pub expected: Vec<Place>,
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
    pub aliased_kills: Vec<AliasedKill>,
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
    /// `p or …` outside `unsafe` on a pointer with no tie (T-0114): the pointer, then the
    /// whole `or`.
    pub untied_conversions: Vec<(Span, Span)>,
    /// A pointer of unknown provenance returned under a tie, outside `unsafe` (T-0114): the
    /// `return`, then the value.
    pub untied_returns: Vec<(Span, Span)>,
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
    /// A tuple's parts, each with the views it holds, kept apart so that reading `t.1`
    /// does not read the view in `t.0`. Empty where the parts are not known, as for a
    /// tuple a call returns, whose views are then unknown if it has any.
    parts: Vec<Value>,
    /// What a safe call returned under a tie its signature declares: trusted as tied, since
    /// the callee's own body is held to that tie (`DZ0419`), whatever its inputs were
    /// (spec §6.7, T-0114).
    vouched: bool,
    /// The proposed placements whose region this value's own memory is in, by index into
    /// [`Regions::sites`] (spec §14.6). Always empty outside [`analyse_regions`], so the
    /// tie analysis proper never sees one; a region is a tie no type declares, which is
    /// why it is followed for a `List<i32>` as much as for a view.
    owned: Vec<usize>,
}

impl Value {
    /// Part `index` of a tuple; an unknown tuple leaves each part unknown too.
    fn part(&self, index: usize) -> Value {
        let mut part = self.parts.get(index).cloned().unwrap_or_default();
        part.unknown |= self.unknown;
        part
    }

    fn origins(&self) -> Vec<&Origin> {
        let mut all: Vec<_> = self.views.iter().chain(&self.places).collect();
        for part in &self.parts {
            all.extend(part.origins());
        }
        all
    }

    fn origins_mut(&mut self) -> Vec<&mut Origin> {
        let Value { places, views, parts, .. } = self;
        let mut all: Vec<_> = views.iter_mut().chain(places.iter_mut()).collect();
        for part in parts {
            all.extend(part.origins_mut());
        }
        all
    }

    /// Every view the value holds, its parts' included: what returning it hands out.
    fn held_views(&self) -> Vec<Origin> {
        let mut all = self.views.clone();
        for part in &self.parts {
            all.extend(part.held_views());
        }
        all
    }

    /// A value kept in a local or held while others are evaluated: only its views
    /// matter, not the place it was read from.
    fn held(self) -> Value {
        let parts = self.parts.into_iter().map(Value::held).collect();
        Value { places: Vec::new(), parts, ..self }
    }

    /// Every region the value's memory is in, its parts' included.
    fn all_owned(&self) -> Vec<usize> {
        let mut all = self.owned.clone();
        for part in &self.parts {
            for site in part.all_owned() {
                if !all.contains(&site) {
                    all.push(site);
                }
            }
        }
        all
    }

    /// The value with no region in it, parts included: a copy (see [`Analysis::passed`]).
    fn without_owned(mut self) -> Value {
        self.owned.clear();
        self.parts = self.parts.into_iter().map(Value::without_owned).collect();
        self
    }
}

/// Why the region check sends a proposed placement to the heap (spec §14.6). The words
/// are `placement`'s, which can name what the program wrote.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// The value is returned, so it outlives every region of the body.
    Returned,
    /// Assigned into this place, which is not rooted at a local of the region's block.
    Stored(ExprId),
    /// Passed by value to this call, builtin or formatting macro, which may keep it.
    Passed(ExprId),
    /// Something still holds the value when the region of this block is released.
    Outlives(BlockId),
    /// Made on every pass of a loop the region outlives.
    EveryPass,
    /// Used where the walk does not follow it.
    Unfollowed(&'static str),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RegionRefusal {
    pub why: Refusal,
    pub at: Span,
}

/// The facts [`analyse_regions`] adds to the walk.
#[derive(Default)]
struct Regions {
    /// Each proposed site's call, and the block whose region it is in.
    sites: Vec<(ExprId, BlockId)>,
    /// Which sites are in the body's result placement (T-0117 stage 2): a region of the
    /// outermost block that its end does not release, since the caller keeps it, and that
    /// a `return` may take.
    results: Vec<bool>,
    by_call: HashMap<ExprId, usize>,
    /// The region blocks being walked, each with its frame in `scopes`.
    active: HashMap<BlockId, usize>,
    /// The frame each local was declared in. A parameter has none: it outlives them all.
    frames: HashMap<LocalId, usize>,
    /// The first refusal of each site.
    refused: Vec<Option<RegionRefusal>>,
    /// Which sites are a by-value parameter's value, in a region of the caller's: like a
    /// result placement, the body's end does not release it, but a `return` may not take it.
    params: Vec<bool>,
    /// Each such parameter, and its site.
    param_sites: Vec<(LocalId, usize)>,
    /// The body's module, which a call must share with its callee to rely on `Staying`.
    module: Option<ModuleId>,
    /// Which sites' values were stored into a container (T-0145): an element taken back
    /// out of the container, or the container lent as `&mut`, carries them.
    stored: Vec<bool>,
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
    /// How many `unsafe` blocks enclose what is walked; an `unsafe fn`'s body starts at one.
    /// Inside, an untied pointer may become a reference (spec §5.2, T-0114).
    unsafe_depth: u32,
    /// Set only by [`analyse_regions`].
    regions: Option<Regions>,
    /// The by-value parameters that stay in the call; set only by [`analyse_regions`].
    staying: Option<&'a Staying>,
    /// The regions a `Box` call's contents hold, from `call` to the `Call` node that adds
    /// them to the box's own value. Empty between calls.
    contents: Vec<usize>,
    /// The first guard local of each `Rc<T>` type in this body, which stands for every value
    /// an `Rc<T>` or `Weak<T>` shares: any two may hold one (D-0094 point 2, T-0124).
    shared: Vec<(Ty, LocalId)>,
}

/// Analyse one checked function, using only other functions' signatures. Findings are
/// available independently of enforcement so std and editor support can land together.
pub fn analyse_body(body: &Body, table: &TypeTable, res: &Resolution) -> BodyAnalysis {
    analyse(body, table, res, None, None).0
}

/// The region check (spec §14.6): the walk again, with each of `sites` (a call, and the
/// block whose region it is proposed for) as a fact, and which of them it refuses, in the
/// order given. Its other findings are dropped: they are [`analyse_body`]'s, and this run
/// must not change what the analysis reports.
///
/// A region is modelled as a tie no type declares. The value a placed call makes holds it,
/// and so does every value that value is moved into: a local, a struct or tuple, an
/// optional, an `or`, a block's value. It dies when its block ends, after the block's own
/// locals drop. Whatever still holds it then, or takes it anywhere a block's end cannot
/// follow — a return, a place not rooted at a local of the block, a call's by-value
/// argument — is refused.
///
/// A copy carries no region: copying a value out of a region builds the copy on the heap
/// (T-0117 step 3, in `List::copy` and `HashMap::copy`), a rule this check trusts as it
/// trusts std's hand-written clauses (§6.7).
///
/// A site marked `true` is in the body's result placement (stage 2), given as the body's
/// outermost block: the same checks, but the block's end does not release it and a
/// `return` may take it, since that is where the caller keeps the result. Anywhere else
/// it would go — a field of `self` or of a parameter, a by-value argument, a value made
/// on every pass of a loop — it is refused as a region would be.
///
/// Each of `params` is a by-value parameter claimed to stay in the call
/// ([`crate::placement::Staying`]): its value is taken to be in a region of the caller's,
/// which outlives the body but which the body may not return, store or pass on. Their
/// refusals follow the sites'. A by-value argument to a parameter of another function
/// that `staying` says stays in the call is not refused: it is gone when the call returns.
pub(crate) fn analyse_regions(
    body: &Body,
    table: &TypeTable,
    res: &Resolution,
    sites: &[(ExprId, BlockId, bool)],
    params: &[LocalId],
    staying: &Staying,
) -> Vec<Option<RegionRefusal>> {
    let mut regions = Regions {
        sites: sites.iter().map(|(call, block, _)| (*call, *block)).collect(),
        results: sites.iter().map(|(_, _, result)| *result).collect(),
        by_call: sites.iter().enumerate().map(|(i, (call, _, _))| (*call, i)).collect(),
        refused: vec![None; sites.len() + params.len()],
        params: vec![false; sites.len()],
        stored: vec![false; sites.len() + params.len()],
        module: Some(crate::placement::owner_module(table, res, &body.owner)),
        ..Regions::default()
    };
    for local in params {
        regions.param_sites.push((*local, regions.sites.len()));
        // No call makes it: the value is the parameter's from the start.
        regions.sites.push((ExprId(u32::MAX), body.root));
        regions.results.push(false);
        regions.params.push(true);
    }
    analyse(body, table, res, Some(regions), Some(staying)).1.map(|r| r.refused).unwrap_or_default()
}

fn analyse(
    body: &Body,
    table: &TypeTable,
    res: &Resolution,
    regions: Option<Regions>,
    staying: Option<&Staying>,
) -> (BodyAnalysis, Option<Regions>) {
    let mut cx = Analysis {
        staying,
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
        unsafe_depth: 0,
        contents: Vec::new(),
        shared: Vec::new(),
        regions,
    };
    let signature = body_signature(body, table, res);
    if signature.as_ref().is_some_and(|s| s.sig.is_unsafe) {
        cx.unsafe_depth = 1;
    }
    let mut external = HashSet::new();
    for local in &body.params {
        let l = body.local(*local);
        // A view taken by value is a view input too: what it views is the caller's
        // (spec §6.7, D-0093).
        if super::points(&l.ty) || l.name == "self" || table.placed_as_view(&l.ty) {
            let origin = Origin::at(Place::local(*local));
            cx.state.insert(*local, Value { views: vec![origin], ..Value::default() });
        }
    }
    // A by-value parameter claimed to stay in the call holds its caller's region.
    let claimed = cx.regions.as_ref().map(|r| r.param_sites.clone()).unwrap_or_default();
    for (local, site) in claimed {
        cx.state.entry(local).or_default().owned.push(site);
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
        match signature.effects(&scope, &bindings) {
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
    // By-value parameters are owned by this body and drop after its local scopes. A view
    // taken by value stands for what it views, which is the caller's and outlives the call;
    // a view of its own storage cannot be returned (`own_storage`).
    for local in body.params.iter().rev() {
        let l = body.local(*local);
        if l.name != "self" && !super::points(&l.ty) && !table.placed_as_view(&l.ty) {
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
    (cx.result, cx.regions)
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
            self.lost(&value.owned, "a field the check cannot follow", self.body.expr(base).span);
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
        // A field of a value in a region is in it too: moving one out takes region memory.
        Value { places, views, unknown, parts: Vec::new(), vouched: false, owned: value.owned }
    }

    /// The local tuple `t.0.1` is a part of, and the indices down to it.
    fn tuple_path(&self, id: ExprId) -> Option<(LocalId, Vec<usize>)> {
        match self.body.expr(id).kind {
            hir::ExprKind::TupleIndex { base, index } => {
                let (local, mut path) = self.tuple_path(base)?;
                path.push(index);
                Some((local, path))
            }
            hir::ExprKind::Local(local) if matches!(self.body.ty(id), Ty::Tuple(_)) => {
                Some((local, Vec::new()))
            }
            _ => None,
        }
    }

    /// `t.0`: the views that part holds, apart from the other parts', and its place in
    /// `t`. The tuple is not read as a whole, so `t.1` is fine after a kill that ended
    /// the view in `t.0`; reading the part is left to the caller, which knows the span.
    fn tuple_part(&mut self, id: ExprId) -> Value {
        let hir::ExprKind::TupleIndex { base, index } = self.body.expr(id).kind else {
            return self.expr(id);
        };
        let by_value = matches!(self.body.ty(base), Ty::Tuple(_));
        let value = match self.body.expr(base).kind {
            hir::ExprKind::Local(local) if by_value => {
                let mut value = self.state.get(&local).cloned().unwrap_or_default();
                value.places = vec![Origin::at(Place::local(local))];
                value
            }
            hir::ExprKind::TupleIndex { .. } => self.tuple_part(base),
            _ => self.expr(base),
        };
        let ty = self.body.ty(id).clone();
        let carries = super::points(&ty) || super::contains_views(&ty, self.table);
        let mut part = if by_value && (!value.parts.is_empty() || !carries) {
            value.part(index)
        } else if by_value && !value.unknown && !value.views.is_empty() {
            // A tuple a call returned is in the call's tie as a whole (§6.7).
            Value { views: value.views.clone(), ..Value::default() }
        } else {
            if carries {
                self.unchecked(
                    self.body.expr(id).span,
                    "tuple field provenance is not modeled yet",
                );
            }
            Value { unknown: carries, ..Value::default() }
        };
        // Only a local tuple's parts are places a kill names: `t.0` inside a parameter's
        // field would be a path no clause can spell.
        let local = matches!(
            self.body.expr(base).kind,
            hir::ExprKind::Local(_) | hir::ExprKind::TupleIndex { .. }
        );
        part.places = if by_value && local {
            value.places.iter().map(|o| o.field(Step::Field(index.to_string()))).collect()
        } else {
            Vec::new()
        };
        part
    }

    /// `origin` with its places spelt as the ties that name them ([`super::canonicalize`]):
    /// `m_data[i]` inside a `List` is `self[*]`, and a `SlotMap`'s second list its
    /// `@items`, as the first is.
    fn canonical(&self, mut origin: Origin) -> Origin {
        let canon = |p: &Place| {
            super::canonical_place(self.table, self.res, &super::root_ty(self.body, p.local), p)
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
                let result = self.invoke(&signature, Some(value), &[], &[], span);
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
                    // and trusted (§6.7): a move from one slot of `[*]` to another, or into a
                    // fresh buffer that becomes `m_data`, would read as killing `self[*]`.
                    // deferred: T-0116 checks these clauses in debug builds instead.
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
        let mut result = self.call_inner(callee, args, expression);
        // An element taken out of a container is in whatever region the values stored
        // into it were (T-0145): `pop`, `remove`, `HashMap::replace`.
        for site in self.taken_out(callee, expression) {
            if !result.owned.contains(&site) {
                result.owned.push(site);
            }
        }
        result
    }

    fn call_inner(&mut self, callee: &Callee, args: &[ExprId], expression: ExprId) -> Value {
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
        let arg_spans: Vec<Span> = args.iter().map(|a| self.body.expr(*a).span).collect();
        let boxed = self.regions.is_some() && crate::placement::boxes_argument(self.res, callee);
        for (i, (arg, value)) in args.iter().zip(&values).enumerate() {
            self.read(value, self.body.expr(*arg).span);
            // What a `Box` is made of is its contents, in the box's own memory: it holds
            // the region as a struct literal holds its fields' (spec §14.6). The call's
            // value carries it, through `contents`.
            if boxed && i == 0 {
                let owned = self.passed(*arg, value.clone()).all_owned();
                self.contents.extend(owned);
                continue;
            }
            // A value given to a call by value is the callee's, which may keep it anywhere:
            // `push`, a constructor, a function of the program's own. A receiver is always
            // borrowed (spec §7.3), and a reference argument carries no region. A parameter
            // that stays in the call does not keep it: it is gone when the call returns.
            if self.stays_in_call(callee, i) {
                continue;
            }
            // Stored into the receiver, a local's container: it lives where that local
            // does (T-0145).
            if let Some(container) = self.stored_into(callee, i) {
                self.region_store(container, *arg, value, expression);
                continue;
            }
            self.sink(*arg, value, Refusal::Passed(expression));
        }
        if let Some(signature) =
            call_signature(self.body, callee, args, expression, self.table, self.res)
        {
            // A default left out is the callee's own expression, which cannot name the
            // caller's places: it kills nothing here, and its provenance is unknown, which
            // `invoke` reports only where a clause or tie depends on that parameter.
            let mut result = self.invoke(&signature, recv.as_ref(), &values, &arg_spans, span);
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
                return self.invoke(&signature, None, &values, &arg_spans, span);
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

    /// Whether `p` is a view struct's own storage, as distinct from what it views (D-0093,
    /// option A of r95): a field holding no view, or the struct itself behind a reference
    /// that `returned` is. A result elided to a view struct, or `in` it whole, is in what it
    /// views, so it may be neither.
    fn own_storage(&self, p: &Place, returned: &Ty) -> bool {
        let ty = &self.body.local(p.local).ty;
        if !self.table.placed_as_view(ty.peeled()) {
            return false;
        }
        match p.steps.first() {
            None => matches!(
                returned,
                Ty::Ref { inner, .. } | Ty::Ptr { inner, .. } if self.table.placed_as_view(inner)
            ),
            Some(Step::Field(name)) => super::field_of(self.table, ty, name)
                .is_some_and(|f| !dz_check::ties::holds_view(self.table, &f, true)),
            Some(_) => false,
        }
    }

    /// What an origin denoting a view struct itself stands for, as a result's input (D-0093,
    /// option A of r95): what the struct views. `None` for an origin that is not a view
    /// struct local, or whose views are not known; it stands for itself then.
    fn what_it_views(&self, origin: &Origin) -> Option<Vec<Origin>> {
        if !origin.target.steps.is_empty() {
            return None;
        }
        let local = origin.target.local;
        if !self.table.placed_as_view(self.body.local(local).ty.peeled()) {
            return None;
        }
        let value = self.state.get(&local)?;
        (!value.unknown && !value.views.is_empty()).then(|| value.views.clone())
    }

    /// What a result `in` a path through a view struct's view field points into (spec
    /// §6.7, D-0093): `tie @items = m_slice` on a `view struct` and `-> &T in self@items`
    /// name what `m_slice` views, so the result is in what the receiver holds, and outlives
    /// the local holding it. `None` where the path is not through such a field, or what the
    /// input holds is not known; the place then stands.
    fn through_view_field(
        &self,
        signature: &Signature,
        scope: &dz_check::ties::Scope<'_>,
        path: &TiePath,
        recv: Option<&Value>,
        args: &[Value],
    ) -> Option<Vec<Origin>> {
        let normalized = super::normalize(scope, path).ok()?;
        let (root_ty, input) = match normalized.root {
            dz_check::ties::Root::SelfValue => (signature.self_ty.clone()?, recv?),
            dz_check::ties::Root::Param(i) => {
                (signature.sig.params.get(i)?.ty.clone(), args.get(i)?)
            }
        };
        let Ty::Named { sym, .. } = root_ty.peeled() else { return None };
        if !self.table.is_view_struct(*sym) {
            return None;
        }
        let Some(Step::Field(name)) = normalized.steps.first() else { return None };
        let field_ty = super::field_of(self.table, &root_ty, name)?;
        if !dz_check::ties::holds_view(self.table, &field_ty, true) {
            return None;
        }
        // A temporary has no place, only the views it holds; a local's are in the state.
        // The struct's views are kept whole, which is its one tie (D-0093, point 3).
        if input.places.is_empty() {
            return (!input.views.is_empty()).then(|| input.views.clone());
        }
        let mut held = Vec::new();
        for origin in &input.places {
            if !origin.target.steps.is_empty() {
                return None;
            }
            let value = self.state.get(&origin.target.local)?;
            if value.unknown || value.views.is_empty() {
                return None;
            }
            held.extend(value.views.iter().cloned());
        }
        Some(held)
    }

    fn invoke(
        &mut self,
        signature: &Signature,
        recv: Option<&Value>,
        args: &[Value],
        arg_spans: &[Span],
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
        // A view taken by value is bound by what it views, as a reference is by what it
        // refers to (D-0093).
        let params = signature
            .sig
            .params
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let view = super::points(&p.ty) || self.table.placed_as_view(&p.ty);
                args.get(i).map(|v| origins(v, view)).unwrap_or_default()
            })
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
            match signature.effects(&scope, &bindings) {
                Ok(effects) => {
                    if effects.incomplete {
                        self.incomplete(span, "a call effect has unknown provenance");
                    }
                    for kill in effects.kills {
                        if !kills.contains(&kill.place) {
                            kills.push(kill.place);
                        }
                    }
                    match &effects.result {
                        ReturnTie::Explicit(_) | ReturnTie::Elided(_) => {
                            // A Dazzle body held to its tie, not C's or an `unsafe fn`'s.
                            value.vouched = !signature.sig.is_unsafe
                                && (signature.sig.has_body || signature.sig.takes_self.is_some());
                            // `in (a, b)`: the result may be in either, so it holds a view
                            // of each, and a kill of any ends it.
                            let paths: Vec<Option<&TiePath>> = match &effects.result {
                                ReturnTie::Explicit(_) => {
                                    signature.sig.ret_ties.iter().map(Some).collect()
                                }
                                _ => vec![None],
                            };
                            for (place, path) in effects.result.places().iter().zip(paths) {
                                // Through a view struct's view field: in what the input
                                // holds, not in the local holding it (spec §6.7).
                                if let Some(held) = path.and_then(|p| {
                                    self.through_view_field(signature, &scope, p, recv, args)
                                }) {
                                    for origin in held {
                                        if !value.views.contains(&origin) {
                                            value.views.push(origin);
                                        }
                                    }
                                    continue;
                                }
                                // In a view struct whole: in what it views (D-0093).
                                let whole = path.map_or(true, |p| {
                                    super::normalize(&scope, p).is_ok_and(|n| n.steps.is_empty())
                                });
                                if let Some(held) = whole
                                    .then(|| self.what_it_views(&Origin::at(place.clone())))
                                    .flatten()
                                {
                                    for origin in held {
                                        if !value.views.contains(&origin) {
                                            value.views.push(origin);
                                        }
                                    }
                                    continue;
                                }
                                let broad = !matches!(place.steps.last(), Some(Step::Storage));
                                let origin = Origin {
                                    target: logical_result(
                                        signature, &scope, &bindings, path, place,
                                    ),
                                    tie: place.clone(),
                                    broad,
                                    payload: None,
                                    dead: None,
                                };
                                if !value.views.contains(&origin) {
                                    value.views.push(origin);
                                }
                            }
                            for root in result_roots(signature, &scope) {
                                let input = match root {
                                    dz_check::ties::Root::SelfValue => &receiver,
                                    dz_check::ties::Root::Param(i) => &params[i],
                                };
                                // A result in an input that is a local view, a `str` say,
                                // is in what that local views too: `v.trim()` dies with
                                // the text `v` points into, not only with `v`. A view
                                // struct stands for what it views alone (D-0093).
                                let mut all = Vec::new();
                                for origin in input {
                                    if let Some(held) = self.what_it_views(origin) {
                                        all.extend(held);
                                        continue;
                                    }
                                    all.push(origin.clone());
                                    if origin.target.steps.is_empty() {
                                        if let Some(v) = self.state.get(&origin.target.local) {
                                            all.extend(v.views.iter().cloned());
                                        }
                                    }
                                }
                                for origin in all {
                                    if !value.views.contains(&origin) {
                                        value.views.push(origin);
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
            // The inputs the clause kills this through, and the others that reach it, by
            // name and argument: the receiver first, then the parameters in order.
            let mut naming: Vec<(String, Option<Span>)> = Vec::new();
            let mut reaching: Vec<(String, Option<Span>)> = Vec::new();
            let reaches = |origins: &[Origin]| origins.iter().any(|o| o.target.covers(&kill));
            if receiver_owns_kill {
                naming.push(("self".into(), None));
            } else if reaches(&receiver) {
                reaching.push(("self".into(), None));
            }
            if !receiver_owns_kill {
                for origin in &receiver {
                    if origin.ended_by(&kill, Reach::Owned) && self.reported.insert(span) {
                        self.result.invalid_uses.push(InvalidUse {
                            span,
                            tie: origin.tie.clone(),
                            next_pass: false,
                            by_drop: false,
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
                let name = signature.sig.params[i].name.clone();
                if owns_kill {
                    naming.push((name, arg_spans.get(i).copied()));
                } else if holds && reaches(origins) {
                    reaching.push((name, arg_spans.get(i).copied()));
                }
                if !owns_kill && holds {
                    for origin in origins {
                        if origin.ended_by(&kill, Reach::Owned) && self.reported.insert(span) {
                            self.result.invalid_uses.push(InvalidUse {
                                span,
                                tie: origin.tie.clone(),
                                next_pass: false,
                                by_drop: false,
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
            // A call may reach what it kills through one argument only (D-0094 point 1): the
            // callee was checked with its parameters taken as separate. Another argument
            // pointing at the owner of what dies (`append_copy(&mut ys, &ys)`), or naming
            // the same kill, would dangle inside it. Memory that cannot die, an `i32`
            // field, is no reason to refuse two arguments.
            if let Some((through, _)) = naming.first() {
                let other = naming.get(1).or_else(|| reaching.first());
                if let Some((other, other_span)) = other {
                    if self.owns_memory(&kill) && self.reported.insert(span) {
                        self.result.aliased_kills.push(AliasedKill {
                            span,
                            killed: kill.clone(),
                            through: through.clone(),
                            other: other.clone(),
                            other_span: *other_span,
                        });
                    }
                }
            }
            self.kill(kill, span, KillKind::Call);
        }
        // Trusted whatever was known of the inputs: the callee's body is held to the tie its
        // signature declares, `in` or elided to its one view input (T-0114).
        let inputs = usize::from(signature.sig.takes_self.is_some())
            + signature
                .sig
                .params
                .iter()
                .filter(|p| dz_check::ties::view_input(self.table, &p.ty))
                .count();
        let declares = !signature.sig.ret_ties.is_empty() || inputs == 1;
        value.vouched |= declares
            && !signature.sig.is_unsafe
            && (signature.sig.has_body || signature.sig.takes_self.is_some());
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
                bounded |= bound(value);
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

    /// The payload of an optional, taken out by value. An optional carries the views of
    /// what it holds where they were followed into it; otherwise a view taken out of one
    /// has unknown provenance.
    fn unwrapped(&mut self, operand: ExprId, value: Value, span: Span) -> Value {
        if let Ty::Optional(inner) = self.body.ty(operand).peeled_refs() {
            if super::contains_views(inner, self.table)
                && value.views.is_empty()
                && value.parts.is_empty()
            {
                self.unchecked(span, "optional view provenance is not modeled yet");
                return Value { unknown: true, ..Value::default() };
            }
        }
        value
    }

    // --- regions (spec §14.6) ---------------------------------------------------------

    /// Whether `e` reads a place by value without moving it, which for a copyable type is
    /// a copy (spec §6.3): `counts.insert(word, 1)`, `let y = x`. `move(x)` is not one,
    /// and a move-only place read by value is `DZ0403` before this runs.
    fn is_copy(&self, e: ExprId) -> bool {
        matches!(
            self.body.expr(e).kind,
            hir::ExprKind::Local(_)
                | hir::ExprKind::Field { .. }
                | hir::ExprKind::TupleIndex { .. }
                | hir::ExprKind::Index { .. }
                | hir::ExprKind::Deref(_)
        ) && self.table.copyable(self.body.ty(e))
    }

    /// `value`, as computed by `e`, where it is taken by value: a copy carries no region,
    /// since copying out of a region builds on the heap (T-0117 step 3). The check trusts
    /// that rule of `List::copy` and `HashMap::copy` as it trusts std's clauses (§6.7).
    fn passed(&self, e: ExprId, value: Value) -> Value {
        if self.regions.is_some() && self.is_copy(e) {
            value.without_owned()
        } else {
            value
        }
    }

    /// Whether argument `arg` of a call to `callee` stays in the call ([`Staying`]).
    fn stays_in_call(&self, callee: &Callee, arg: usize) -> bool {
        let (Some(staying), Some(module)) =
            (self.staying, self.regions.as_ref().and_then(|r| r.module))
        else {
            return false;
        };
        staying.stays(self.res, module, callee, arg)
    }

    /// The local whose container argument `arg` of a call to `callee` is stored into, when
    /// the callee stores it into its receiver ([`Staying::stores`]) and the receiver is a
    /// by-value path from a local (T-0145).
    fn stored_into(&self, callee: &Callee, arg: usize) -> Option<LocalId> {
        let (staying, module) = (self.staying?, self.regions.as_ref()?.module?);
        staying.stores(self.res, module, callee, arg)?;
        let Callee::Method { recv, .. } = callee else { return None };
        self.local_root(*recv)
    }

    /// `value`, argument `arg` of `call`, stored into the container in `container`: it may
    /// go only into one declared inside each of its regions' blocks, as for an assignment
    /// ([`Self::region_assign`]), and the container then holds it.
    fn region_store(&mut self, container: LocalId, arg: ExprId, value: &Value, call: ExprId) {
        let owned = self.passed(arg, value.clone()).all_owned();
        if owned.is_empty() {
            return;
        }
        let span = self.body.expr(call).span;
        let Some(regions) = &mut self.regions else { return };
        let declared = regions.frames.get(&container).copied();
        let refused: Vec<usize> = owned
            .iter()
            .copied()
            .filter(|site| match (declared, regions.active.get(&regions.sites[*site].1)) {
                (Some(at), Some(frame)) => at < *frame,
                _ => true,
            })
            .collect();
        for site in &owned {
            if let Some(s) = regions.stored.get_mut(*site) {
                *s = true;
            }
        }
        let recv = match &self.body.expr(call).kind {
            hir::ExprKind::Call { callee: Callee::Method { recv, .. }, .. } => *recv,
            _ => call,
        };
        self.refuse(&refused, Refusal::Stored(recv), span);
        let v = self.state.entry(container).or_default();
        for site in owned {
            if !v.owned.contains(&site) {
                v.owned.push(site);
            }
        }
    }

    /// The regions stored into the container a call takes an element out of: a `&mut self`
    /// method whose result owns memory, on a by-value path from a local (T-0145).
    fn taken_out(&self, callee: &Callee, call: ExprId) -> Vec<usize> {
        let Some(regions) = &self.regions else { return Vec::new() };
        let Callee::Method { recv, owner, name, .. } = callee else { return Vec::new() };
        let mutates = self
            .table
            .methods_sig(*owner)
            .and_then(|s| s.methods.get(name))
            .is_some_and(|sig| sig.takes_self == Some(true));
        if !mutates || !crate::storing::owns(self.table, self.body.ty(call)) {
            return Vec::new();
        }
        let Some(container) = self.local_root(*recv) else { return Vec::new() };
        let Some(v) = self.state.get(&container) else { return Vec::new() };
        v.owned.iter().copied().filter(|s| regions.stored.get(*s) == Some(&true)).collect()
    }

    /// `&mut place`: what was stored into a container under it is lost (T-0145).
    fn lend(&mut self, place: ExprId, span: Span) {
        let Some(regions) = &self.regions else { return };
        let Some(container) = self.local_root(place) else { return };
        let Some(v) = self.state.get(&container) else { return };
        let lost: Vec<usize> =
            v.owned.iter().copied().filter(|s| regions.stored.get(*s) == Some(&true)).collect();
        self.lost(&lost, "lent as `&mut` once stored", span);
    }

    /// Whether the site `call` may be made on every pass of a loop its region outlives
    /// (T-0145): its value, or a value holding it, is the argument a method keeps on every
    /// call, stored into a local's container that keeps everything stored into it
    /// ([`crate::storing::keeps`]). The region then grows only as the container does.
    fn kept_each_pass(&self, call: ExprId) -> bool {
        let (Some(staying), Some(module)) =
            (self.staying, self.regions.as_ref().and_then(|r| r.module))
        else {
            return false;
        };
        let body = self.body;
        let mut parent: HashMap<ExprId, ExprId> = HashMap::new();
        for i in 0..body.exprs.len() as u32 {
            for c in crate::storing::children(body, ExprId(i)) {
                parent.insert(c, ExprId(i));
            }
        }
        let mut top = call;
        while let Some(&p) = parent.get(&top) {
            let holds = match &body.expr(p).kind {
                hir::ExprKind::Move(_)
                | hir::ExprKind::StructLit { .. }
                | hir::ExprKind::UnionVariant { .. }
                | hir::ExprKind::Tuple(_)
                | hir::ExprKind::ArrayLit(_) => true,
                hir::ExprKind::Or { operand, clause } => {
                    *operand == top || matches!(clause, hir::OrClause::Value(v) if *v == top)
                }
                hir::ExprKind::Call { callee, args } => {
                    args.first() == Some(&top) && crate::placement::boxes_argument(self.res, callee)
                }
                _ => false,
            };
            if !holds {
                break;
            }
            top = p;
        }
        let Some(&p) = parent.get(&top) else { return false };
        let hir::ExprKind::Call { callee, args } = &body.expr(p).kind else { return false };
        let Some(i) = args.iter().position(|a| *a == top) else { return false };
        if staying.stores(self.res, module, callee, i) != Some(true) {
            return false;
        }
        let Callee::Method { recv, .. } = callee else { return false };
        self.local_root(*recv).is_some_and(|l| crate::storing::keeps(body, self.table, l))
    }

    /// `value`, computed by `e`, goes where the walk does not follow it: every region in it
    /// is refused, unless it is a copy.
    fn sink(&mut self, e: ExprId, value: &Value, why: Refusal) {
        if self.regions.is_none() || self.is_copy(e) {
            return;
        }
        let owned = value.all_owned();
        self.refuse(&owned, why, self.body.expr(e).span);
    }

    /// Regions lost where the walk gives up on a value.
    fn lost(&mut self, owned: &[usize], what: &'static str, at: Span) {
        self.refuse(owned, Refusal::Unfollowed(what), at);
    }

    fn refuse(&mut self, owned: &[usize], why: Refusal, at: Span) {
        let Some(regions) = &mut self.regions else { return };
        for site in owned {
            if let Some(slot @ None) = regions.refused.get_mut(*site) {
                *slot = Some(RegionRefusal { why: why.clone(), at });
            }
        }
    }

    /// A call's result: in its region when the call is a proposed site. A site that can
    /// run more than once while its region lives is refused (T-0117's loop rule): that is
    /// one inside a loop its region's block encloses, such as a `while` condition.
    fn region_site(&mut self, call: ExprId, mut value: Value) -> Value {
        let Some(regions) = &self.regions else { return value };
        let Some(&site) = regions.by_call.get(&call) else { return value };
        let block = regions.sites[site].1;
        let span = self.body.expr(call).span;
        match regions.active.get(&block).copied() {
            None => self.refuse(&[site], Refusal::Unfollowed("outside its region's block"), span),
            Some(frame) => {
                if self.loops.last().is_some_and(|l| frame < l.scope_depth)
                    && !self.kept_each_pass(call)
                {
                    self.refuse(&[site], Refusal::EveryPass, span);
                }
            }
        }
        value.owned.push(site);
        value
    }

    /// `target = value`: a region may go only into a place rooted at a local declared
    /// inside its block, reached without a pointer, a reference or an index. Anything else
    /// — a field of `self` or of a parameter, an element, a static — outlives the block.
    fn region_assign(&mut self, target: ExprId, op: hir::AssignOp, value: &Value, span: Span) {
        if self.regions.is_none() {
            return;
        }
        let owned = value.all_owned();
        let whole = op == hir::AssignOp::Assign
            && matches!(self.body.expr(target).kind, hir::ExprKind::Local(_));
        let root = self.local_root(target);
        if owned.is_empty() {
            // The old value, and whatever region it was in, is gone.
            if let (true, Some(local)) = (whole, root) {
                if let Some(v) = self.state.get_mut(&local) {
                    v.owned.clear();
                }
            }
            return;
        }
        if op != hir::AssignOp::Assign {
            self.lost(&owned, "used by an operator", span);
            return;
        }
        let regions = self.regions.as_ref().unwrap();
        let declared = root.and_then(|l| regions.frames.get(&l).copied());
        // A constructor building a field of the value it makes (round 3): what is in the
        // result placement may go there, since the value lives where it does. `self`
        // then holds it, so moving the field out later is followed like any other value.
        let into_self = root.is_none()
            && matches!(self.body.owner, BodyOwner::Ctor { .. })
            && crate::placement::self_field(self.body, target);
        let refused: Vec<usize> = owned
            .iter()
            .copied()
            .filter(|site| {
                if into_self && regions.results[*site] {
                    return false;
                }
                let block = regions.sites[*site].1;
                match (declared, regions.active.get(&block)) {
                    (Some(at), Some(frame)) => at < *frame,
                    _ => true,
                }
            })
            .collect();
        self.refuse(&refused, Refusal::Stored(target), span);
        let root = if into_self { self.self_local() } else { root };
        if let Some(local) = root {
            let v = self.state.entry(local).or_default();
            if whole {
                v.owned.clear();
            }
            for site in owned {
                if !v.owned.contains(&site) {
                    v.owned.push(site);
                }
            }
        }
    }

    /// The body's `self`, if it has one.
    fn self_local(&self) -> Option<LocalId> {
        self.body.params.iter().copied().find(|p| self.body.local(*p).name == "self")
    }

    /// The local a place is a field path of, by value all the way: none through a
    /// reference, a pointer or an index, and none for `self`, a parameter or a static.
    fn local_root(&self, e: ExprId) -> Option<LocalId> {
        match self.body.expr(e).kind {
            hir::ExprKind::Local(local) => {
                let l = self.body.local(local);
                let own = l.name != "self"
                    && !self.body.params.contains(&local)
                    && !matches!(l.ty, Ty::Ref { .. } | Ty::Ptr { .. } | Ty::OwnPtr { .. });
                own.then_some(local)
            }
            hir::ExprKind::Field { base, .. } | hir::ExprKind::TupleIndex { base, .. } => {
                if matches!(self.body.ty(base), Ty::Ref { .. } | Ty::Ptr { .. } | Ty::OwnPtr { .. })
                {
                    return None;
                }
                self.local_root(base)
            }
            _ => None,
        }
    }

    /// `block`'s region is released: anything outside it still holding the region — a
    /// local of an enclosing block, a value being evaluated, the block's own value — would
    /// be used or dropped after its memory is gone.
    fn region_end(&mut self, block: BlockId, frame: usize, value: Option<&Value>) {
        let Some(regions) = &self.regions else { return };
        // A result placement is the caller's, and outlives every block of the body.
        let mine: Vec<usize> = (0..regions.sites.len())
            .filter(|s| regions.sites[*s].1 == block && !regions.results[*s] && !regions.params[*s])
            .collect();
        let mut found = Vec::new();
        let mut take = |v: &Value| {
            for site in v.all_owned() {
                if mine.contains(&site) && !found.contains(&site) {
                    found.push(site);
                }
            }
        };
        for (local, v) in &self.state {
            // A binding made inside the block dies with it.
            if regions.frames.get(local).is_some_and(|f| *f >= frame) {
                continue;
            }
            take(v);
        }
        for v in &self.held {
            take(v);
        }
        if let Some(v) = value {
            take(v);
        }
        let span = self.body.block(block).span;
        self.refuse(&found, Refusal::Outlives(block), span);
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
        if let Some(origin) = value.origins().into_iter().find(|o| o.dead.is_some()) {
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
                by_drop: false,
                killed: origin.dead.clone().unwrap(),
            });
            self.reported.insert(span);
        }
    }

    /// `local` is dropped at `span`. When its type has a `drop` and holds views, the drop
    /// may read them, so they are used here although no line shows it: Rust's drop check
    /// (spec §6.7, T-0127). A type with no `drop` of its own, such as an iterator, loses
    /// nothing.
    fn drop_reads(&mut self, local: LocalId, span: Span) {
        let ty = &self.body.local(local).ty;
        if !self.table.needs_drop(ty) || !super::contains_views(ty, self.table) {
            return;
        }
        let Some(value) = self.state.get(&local).cloned() else { return };
        let before = self.result.invalid_uses.len();
        self.read(&value, span);
        for u in &mut self.result.invalid_uses[before..] {
            u.by_drop = true;
        }
    }

    /// The value a guarded `Box` holds, as views of its `@value`, when `init` is the guard
    /// of a `Box` named by a place: `h.node`, a local, or one through a reference. `None`
    /// for anything else, whose target stays a view of its guard. A place expression is
    /// walked again here, which reads but changes nothing.
    fn box_handle(&mut self, init: ExprId) -> Option<Vec<Origin>> {
        let hir::ExprKind::Call { callee: Callee::Method { recv, .. }, .. } =
            &self.body.expr(init).kind
        else {
            return None;
        };
        let recv = *recv;
        let boxed = self.res.lookup(self.res.prelude, "Box");
        if !matches!(self.body.ty(recv).peeled_refs(), Ty::Named { sym, .. } if Some(*sym) == boxed)
        {
            return None;
        }
        fn place(body: &Body, e: ExprId) -> bool {
            match body.expr(e).kind {
                hir::ExprKind::Local(_) => true,
                hir::ExprKind::Field { base, .. } | hir::ExprKind::TupleIndex { base, .. } => {
                    place(body, base)
                }
                _ => false,
            }
        }
        if !place(self.body, recv) {
            return None;
        }
        let value = self.expr(recv);
        let ptr = Step::Field(super::canonical_field(self.table, boxed?, "m_ptr"));
        let views: Vec<Origin> = self
            .targets(recv, &value)
            .into_iter()
            .map(|o| {
                // As `&xs[i]` is: the value, `@value`, kept valid by the allocation, which
                // a write, `null` or a move of the box ends (spec §6.7).
                let origin = Origin {
                    target: o.target.followed_by(&[ptr.clone(), Step::Storage, Step::Elements]),
                    tie: o.target.followed_by(&[ptr.clone(), Step::Storage]),
                    broad: false,
                    payload: None,
                    dead: o.dead,
                };
                self.canonical(origin)
            })
            .collect();
        (!views.is_empty()).then_some(views)
    }

    /// The place standing for every value an `Rc<T>` shares, when `guard` holds an `Rc<T>`:
    /// the first such guard in this body (D-0094 point 2). Guards are never dropped by the
    /// walk, so the place lives as long as the body.
    fn shared_root(&mut self, guard: LocalId) -> Option<LocalId> {
        let ty = self.body.local(guard).ty.clone();
        let rc = self.res.lookup(self.res.prelude, "Rc");
        if !matches!(&ty, Ty::Named { sym, .. } if Some(*sym) == rc) {
            return None;
        }
        if let Some((_, root)) = self.shared.iter().find(|(t, _)| *t == ty) {
            return Some(*root);
        }
        self.shared.push((ty, guard));
        Some(guard)
    }

    /// Whether killing `place` can free or move memory, or change a variant: an allocation
    /// behind a tie, or a value that owns one. Killing an `i32` field changes nothing a
    /// view can lose (D-0094 point 1). A place whose type cannot be followed counts as one.
    fn owns_memory(&self, place: &Place) -> bool {
        let mut ty = super::root_ty(self.body, place.local);
        for step in &place.steps {
            match step {
                // `field_of` looks through references and pointers to the struct.
                Step::Field(name) => match super::field_of(self.table, &ty, name) {
                    Some(t) => ty = t,
                    None => return true,
                },
                Step::Elements | Step::Storage => return true,
            }
        }
        match ty.peeled_refs() {
            Ty::Ptr { .. } | Ty::OwnPtr { .. } => true,
            t => moves_memory(t, self.table),
        }
    }

    fn kill(&mut self, place: Place, span: Span, kind: KillKind) {
        let killed = BodyKill { place, span, kind };
        for value in self.state.values_mut().chain(&mut self.held) {
            for origin in value.origins_mut() {
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
        let owned = value.owned.clone();
        self.bind_views(local, value);
        if let Some(regions) = &mut self.regions {
            // A binding lives in the frame it is made in: a `let`'s block, or the one around
            // an `if let`, a `for` or a formatting macro.
            if !self.body.params.contains(&local) {
                let frame = self.scopes.len().saturating_sub(1);
                regions.frames.entry(local).or_insert(frame);
            }
            // Whatever its type: a `List<i32>` holds no view, but it may hold a region.
            if !owned.is_empty() {
                self.state.entry(local).or_default().owned = owned;
            } else if let Some(v) = self.state.get_mut(&local) {
                v.owned.clear();
            }
        }
    }

    fn bind_views(&mut self, local: LocalId, value: Value) {
        if super::points(&self.body.local(local).ty) {
            self.state.insert(local, Value { places: Vec::new(), ..value });
        } else if !value.parts.is_empty() {
            // A tuple, or an optional holding one, keeps each part's views apart.
            if value.unknown {
                self.unchecked(
                    self.body.local(local).span,
                    "aggregate view bindings are not modeled yet",
                );
            }
            self.state.insert(local, value.held());
        } else if !value.unknown
            && !value.views.is_empty()
            && super::contains_views(&self.body.local(local).ty, self.table)
        {
            // A struct holding a view a call returned (`xs.iter()`'s `SliceIter`) is in
            // that call's tie, so reading the local after a kill of it is a use (§6.7).
            self.state.insert(local, value.held());
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
        let marked = self.body.unsafe_blocks.contains(&block);
        self.unsafe_depth += u32::from(marked);
        let value = self.block_inner(block);
        self.unsafe_depth -= u32::from(marked);
        value
    }

    /// Whether `value`, a pointer, has a tie a caller can trust: a view of a place, or what
    /// a safe call returned with `in`. A pointer parameter, a pointer field with no tie, a
    /// cast and C's pointers are untied (spec §5.2, §6.7, T-0114).
    fn tied_pointer(&self, value: &Value) -> bool {
        value.vouched
            || !value.unknown
                && !value.views.is_empty()
                && value.views.iter().all(|o| {
                    !(o.target.steps.is_empty()
                        && self.body.params.contains(&o.target.local)
                        && super::or_pointer(&self.body.local(o.target.local).ty))
                })
    }

    /// An expression written `unsafe { … }`, whose value the programmer vouches for.
    fn vouched(&self, e: ExprId) -> bool {
        matches!(self.body.expr(e).kind, hir::ExprKind::Block(b) if self.body.unsafe_blocks.contains(&b))
    }

    fn block_inner(&mut self, block: BlockId) -> Value {
        let frame = self.scopes.len();
        if let Some(regions) = &mut self.regions {
            if regions.sites.iter().any(|(_, b)| *b == block) {
                regions.active.insert(block, frame);
            }
        }
        let value = self.block_scope(block);
        // The region is released after everything in the block has dropped (spec §14.6).
        if self.regions.as_ref().is_some_and(|r| r.active.contains_key(&block)) {
            self.region_end(block, frame, Some(&value));
            if let Some(regions) = &mut self.regions {
                regions.active.remove(&block);
            }
        }
        value
    }

    fn block_scope(&mut self, block: BlockId) -> Value {
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
            self.drop_reads(local, self.body.local(local).span);
            self.kill(Place::local(local), self.body.block(block).span, KillKind::Drop);
            self.state.remove(&local);
            for origin in value.origins_mut() {
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
                let value = match init {
                    Some(e) => self.passed(e, value),
                    None => value,
                };
                if let Some(local) = local {
                    self.bind(local, value);
                    self.scopes.last_mut().unwrap().0.push(local);
                }
            }
            hir::StmtKind::LetTuple { locals, init } => {
                // Each name takes its own part's views, as `t.0` would.
                let value = self.expr(init);
                let value = self.passed(init, value);
                let known = !value.parts.is_empty();
                // A tuple a call returns is in the call's tie as a whole (§6.7), so each
                // part that can hold a view may point anywhere the whole does.
                let whole = !known && !value.unknown && !value.views.is_empty();
                if !known && !whole && super::contains_views(self.body.ty(init), self.table) {
                    self.unchecked(node.span, "tuple view destructuring is not modeled yet");
                }
                for (i, local) in locals.into_iter().enumerate() {
                    if let Some(local) = local {
                        let part = if known {
                            value.part(i)
                        } else if whole {
                            Value { views: value.views.clone(), ..Value::default() }
                        } else {
                            Value { unknown: true, ..Value::default() }
                        };
                        // A region held by the tuple as a whole may be any part's.
                        let mut part = part;
                        for site in &value.owned {
                            if !part.owned.contains(site) {
                                part.owned.push(*site);
                            }
                        }
                        self.bind(local, part);
                        self.scopes.last_mut().unwrap().0.push(local);
                    }
                }
            }
            hir::StmtKind::Assign { target, op, value: value_expr, bind } => {
                // The emitter evaluates the RHS before the destination.
                let value = self.expr(value_expr);
                let value = self.passed(value_expr, value);
                // A constructor binding a reference field: nothing was there to kill, and the
                // struct being built is in what the constructor's elided result names
                // (§6.7).
                if bind {
                    self.lost(&value.all_owned(), "a reference field", node.span);
                    return Value::default();
                }
                self.region_assign(target, op, &value, node.span);
                let ty = self.body.ty(target);
                if op == hir::AssignOp::Assign {
                    if let hir::ExprKind::Local(local) = self.body.expr(target).kind {
                        if super::points(ty) && !matches!(ty, Ty::Ref { .. }) {
                            // Pointer/slice/str assignment retargets the local; assigning
                            // to a reference writes the referent instead.
                            self.bind(local, value);
                            return Value::default();
                        }
                        if !super::points(ty) && super::contains_views(ty, self.table) {
                            // A struct or tuple holding views takes the new value's; the
                            // old ones are read only by the old value's `drop`, and what
                            // the old value owned dies.
                            self.drop_reads(local, node.span);
                            if moves_memory(ty, self.table) {
                                self.kill(Place::local(local), node.span, KillKind::Assign);
                            }
                            self.bind(local, value);
                            return Value::default();
                        }
                    }
                    // So does assigning one of a local tuple's parts: `t.0 = b.first()`.
                    if super::points(ty) && !matches!(ty, Ty::Ref { .. }) {
                        if let Some((local, path)) = self.tuple_path(target) {
                            let part = self.state.get_mut(&local).and_then(|value| {
                                path.iter().try_fold(value, |v, &i| v.parts.get_mut(i))
                            });
                            if let Some(part) = part {
                                *part = value.held();
                                return Value::default();
                            }
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
                self.drop_reads(local, self.body.local(local).span);
                self.kill(Place::local(local), self.body.local(local).span, KillKind::Drop);
                self.state.remove(&local);
            }
            // A jump out of a region's block releases the region too.
            let released = self
                .regions
                .as_ref()
                .and_then(|r| r.active.iter().find(|(_, f)| **f == index).map(|(b, _)| *b));
            if let Some(block) = released {
                self.region_end(block, index, None);
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
            hir::ExprKind::Ref { place, mutable, .. } => {
                let value = self.expr(place);
                // Lent as `&mut`, a container may have an element taken out where the walk
                // cannot see it, so what was stored into it is lost (T-0145).
                if mutable {
                    self.lend(place, node.span);
                }
                // A reference to a temporary view struct has no place to point at; it
                // stands for what the temporary views, which is all a result given it may
                // be in (spec §6.7, D-0093).
                let views =
                    if value.places.is_empty() && self.table.placed_as_view(self.body.ty(place)) {
                        value.views
                    } else {
                        value.places
                    };
                Value { views, unknown: value.unknown, ..Value::default() }
            }
            hir::ExprKind::Deref(inner) => {
                let value = self.expr(inner);
                Value { places: value.views, unknown: value.unknown, ..Value::default() }
            }
            hir::ExprKind::Field { base, index } => self.field(base, index),
            hir::ExprKind::TupleIndex { .. } => {
                let value = self.tuple_part(id);
                self.read(&value, node.span);
                value
            }
            hir::ExprKind::Index { base, index } => {
                let value = self.expr(base);
                let at = self.expr(index);
                self.sink(index, &at, Refusal::Unfollowed("used as an index"));
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
                    // Moved out, it is not dropped here: its views went with the value.
                    self.state.remove(&local);
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
            hir::ExprKind::Call { callee, args } => {
                let mut value = self.call(&callee, &args, id);
                for site in std::mem::take(&mut self.contents) {
                    if !value.owned.contains(&site) {
                        value.owned.push(site);
                    }
                }
                self.region_site(id, value)
            }
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
                let bound = self.passed(operand, bound);
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
                // Only a tied pointer becomes a reference safely; an untied one is the
                // programmer's word, in `unsafe` (spec §5.2, D-0093 point 11).
                if super::or_pointer(self.body.ty(operand))
                    && self.unsafe_depth == 0
                    && !self.vouched(operand)
                    && !self.tied_pointer(&value)
                {
                    self.result.untied_conversions.push((self.body.expr(operand).span, node.span));
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
                if let Some(returned_id) = value {
                    let returned_ty = self.body.ty(returned_id).clone();
                    let value = self.expr(returned_id);
                    // `return x` moves a local out in C++ even where Dazzle copies it, so a
                    // local returned is never a copy here.
                    let kept =
                        if matches!(self.body.expr(returned_id).kind, hir::ExprKind::Local(_)) {
                            value.all_owned()
                        } else {
                            self.passed(returned_id, value.clone()).all_owned()
                        };
                    // What is in the result placement is where the caller wants it.
                    let kept: Vec<usize> = match &self.regions {
                        Some(r) => kept.into_iter().filter(|s| !r.results[*s]).collect(),
                        None => kept,
                    };
                    self.refuse(&kept, Refusal::Returned, node.span);
                    // A tuple hands out the views in its parts as well.
                    let returned = value.held_views();
                    // Scope-exit kills are applied below; returning a view of a local
                    // must be diagnosed before that local disappears from the state.
                    for origin in &returned {
                        let local = self.body.local(origin.target.local);
                        let caller_owned = self.body.params.contains(&origin.target.local)
                            && (super::points(&local.ty)
                                || local.name == "self"
                                || self.table.placed_as_view(&local.ty));
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
                                    by_drop: false,
                                    killed,
                                });
                            }
                        }
                    }
                    if !self.reported.contains(&node.span) {
                        match &self.expected_return {
                            ReturnTie::Explicit(_) | ReturnTie::Elided(_) => {
                                let expected = self.expected_return.places();
                                // A result in a view struct whole is in what it views, so
                                // its own storage is not covered by it (D-0093).
                                let covered = |p: &Place| {
                                    expected.iter().any(|e| {
                                        e.covers(p)
                                            && !(e.steps.is_empty()
                                                && self.own_storage(p, &returned_ty))
                                    })
                                };
                                // Spelt as the ties that name them, as the signature's
                                // `in` is: `self.m_books[*]` is `self[*]` (§6.7).
                                let views: Vec<Origin> =
                                    value.views.iter().map(|o| self.canonical(o.clone())).collect();
                                if let Some(origin) = views.iter().find(|o| {
                                    !views.iter().any(|other| {
                                        other.target == o.target
                                            && (covered(&other.tie) || covered(&other.target))
                                    })
                                }) {
                                    self.result.invalid_returns.push(InvalidReturn {
                                        span: node.span,
                                        expected: expected.to_vec(),
                                        actual: origin.tie.clone(),
                                    });
                                    self.reported.insert(node.span);
                                }
                                if value.unknown {
                                    // A pointer whose tie is unknown, returned under one,
                                    // would make it look tied to the caller: an untied one
                                    // laundered into a safe `or` (T-0114).
                                    if super::or_pointer(&returned_ty)
                                        && self.unsafe_depth == 0
                                        && !self.vouched(returned_id)
                                    {
                                        let at = self.body.expr(returned_id).span;
                                        self.result.untied_returns.push((node.span, at));
                                    }
                                    self.unchecked(
                                        node.span,
                                        "the returned view's provenance is unknown",
                                    );
                                }
                            }
                            ReturnTie::Unknown if !returned.is_empty() => self.unchecked(
                                node.span,
                                "the returned view has no unambiguous declared tie",
                            ),
                            _ => {}
                        }
                    }
                    // A result returned by value moves out before the locals drop; only
                    // its views must survive the drops and defers that follow.
                    self.held.push(value.held());
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
            hir::ExprKind::Guarded { guard, init: init_expr, value } => {
                let handle = self.box_handle(init_expr);
                let init = self.expr(init_expr);
                self.sink(init_expr, &init, Refusal::Unfollowed("held by a guard"));
                self.bind(guard, init.without_owned());
                let mut target = self.expr(value);
                if let Some(views) = handle {
                    // A `Box`'s value is in the box (D-0094 point 3): writing, emptying or
                    // moving the box ends it, as assigning a `String` ends its text.
                    target.views = views;
                    target.unknown = false;
                } else if let Some(root) = self.shared_root(guard) {
                    // Any two `Rc<T>`s may share one value, so every one's is one place.
                    for origin in &mut target.views {
                        if origin.target.local == guard {
                            origin.target.local = root;
                        }
                        if origin.tie.local == guard {
                            origin.tie.local = root;
                        }
                    }
                }
                target
            }
            hir::ExprKind::Formatted { args, params, body } => {
                for (arg, param) in args.into_iter().zip(params) {
                    let value = self.expr(arg);
                    // The macro's body is a C++ lambda its arguments are passed to by value.
                    self.sink(arg, &value, Refusal::Passed(id));
                    self.bind(param, value.without_owned());
                }
                self.block(body)
            }
            hir::ExprKind::Unary { operand, .. } => {
                let value = self.expr(operand);
                self.sink(operand, &value, Refusal::Unfollowed("used by an operator"));
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
                let left = self.expr(lhs);
                self.sink(lhs, &left, Refusal::Unfollowed("used by an operator"));
                if !self.live {
                    return Value::default();
                }
                if matches!(op, hir::BinaryOp::And | hir::BinaryOp::Or) {
                    let entry = self.state.clone();
                    let held_entry = self.held.clone();
                    let scope_entry = self.scopes.clone();
                    let right = self.expr(rhs);
                    self.sink(rhs, &right, Refusal::Unfollowed("used by an operator"));
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
                    let right = self.expr(rhs);
                    self.sink(rhs, &right, Refusal::Unfollowed("used by an operator"));
                }
                Value::default()
            }
            hir::ExprKind::Range { start, end, .. } => {
                for e in [start, end].into_iter().flatten() {
                    let value = self.expr(e);
                    self.sink(e, &value, Refusal::Unfollowed("used by an operator"));
                }
                Value::default()
            }
            hir::ExprKind::StructLit { fields, .. }
            | hir::ExprKind::UnionVariant { payload: fields, .. } => {
                // The struct holds what is moved into it, region and all.
                let mut owned = Vec::new();
                for e in fields.into_iter().flatten() {
                    let value = self.expr(e);
                    for site in self.passed(e, value).all_owned() {
                        if !owned.contains(&site) {
                            owned.push(site);
                        }
                    }
                }
                Value { owned, ..Value::default() }
            }
            hir::ExprKind::Tuple(parts) => {
                // Each part is held while the next is evaluated, as a call's arguments
                // are, so a kill in a later part ends a view in an earlier one.
                let held_start = self.held.len();
                for e in parts {
                    let value = self.expr(e);
                    let value = self.passed(e, value);
                    self.held.push(value.held());
                }
                let parts = self.held.split_off(held_start);
                if !self.live {
                    return Value::default();
                }
                Value { parts, ..Value::default() }
            }
            hir::ExprKind::ArrayLit(parts) => {
                let mut owned = Vec::new();
                for e in parts {
                    let value = self.expr(e);
                    for site in self.passed(e, value).all_owned() {
                        if !owned.contains(&site) {
                            owned.push(site);
                        }
                    }
                }
                if super::contains_views(&node.ty, self.table) {
                    self.unchecked(node.span, "aggregate view provenance is not modeled yet");
                }
                Value { owned, ..Value::default() }
            }
            hir::ExprKind::Builtin { name, args } | hir::ExprKind::Intrinsic { name, args } => {
                let mut first = Value::default();
                for (i, arg) in args.into_iter().enumerate() {
                    let value = self.expr(arg);
                    // `construct(dst, …)`, `destroy(dst)` and `move_out(dst)` act on `dst`
                    // in place; anything else given to a builtin may be kept by it.
                    let in_place =
                        i == 0 && matches!(name.as_str(), "construct" | "destroy" | "move_out");
                    if !in_place {
                        self.sink(arg, &value, Refusal::Passed(id));
                    }
                    if i == 0 && matches!(name.as_str(), "destroy" | "move_out") {
                        self.kill_places(&value.places, node.span, KillKind::Drop);
                        if value.places.is_empty() {
                            self.incomplete(node.span, "destruction through unknown storage");
                        }
                    }
                    if i == 0 {
                        first = value;
                    }
                }
                // `str::from_utf8(bytes)` is the bytes, as text: it views what they view.
                if name == "str_from_utf8" {
                    return Value {
                        views: first.held_views(),
                        unknown: first.unknown,
                        ..Value::default()
                    };
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
        // A tuple whose parts were not followed (a parameter's, a field's) holds views
        // from anywhere, which a merge with one whose parts are known must not lose. One a
        // call returned is in the call's tie as a whole, and its views say so.
        let tuple = match &node.ty {
            Ty::Optional(inner) => matches!(**inner, Ty::Tuple(_)),
            ty => matches!(ty, Ty::Tuple(_)),
        };
        if tuple
            && value.parts.is_empty()
            && value.views.is_empty()
            && super::contains_views(&node.ty, self.table)
        {
            value.unknown = true;
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
    for site in &from.owned {
        if !to.owned.contains(site) {
            to.owned.push(*site);
        }
    }
    to.unknown |= from.unknown;
    to.vouched &= from.vouched;
    if to.parts.is_empty() {
        to.parts = from.parts.clone();
    } else {
        merge_held(&mut to.parts, &from.parts);
    }
}

/// Cut a loop's provenance down to the analysis bound, parts included; whether it had to.
fn bound(value: &mut Value) -> bool {
    let mut bounded = false;
    for origins in [&mut value.places, &mut value.views] {
        if origins.len() > 64
            || origins.iter().any(|o| o.target.steps.len() > 64 || o.tie.steps.len() > 64)
        {
            origins.truncate(64);
            origins.retain(|o| o.target.steps.len() <= 64 && o.tie.steps.len() <= 64);
            bounded = true;
            value.unknown = true;
        }
    }
    for part in &mut value.parts {
        bounded |= bound(part);
    }
    bounded
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
    /// [`super::call_effects`]. A result that is a view only through a plain `T` needs no
    /// exemption since T-0109: no plain `T` is a view (D-0093, point 8).
    pub(super) fn effects(
        &self,
        scope: &dz_check::ties::Scope<'_>,
        bindings: &Bindings,
    ) -> Result<super::CallEffects, dz_check::ties::Error> {
        super::call_effects(scope, bindings)
    }

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
        // A body reads its own signature unsubstituted, where `call_effects` already
        // leaves a `T` result unknown.
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
        // A constructor's signature returns nothing, since it fills in `self`; called, it
        // makes the struct, which a view struct's constructor elides to its one view input,
        // as a function returning it would (spec §6.7).
        Callee::Ctor { owner, index } => (
            {
                let mut sig = table.struct_sig(*owner)?.ctors.get(*index)?.clone();
                if table.is_view_struct(*owner) {
                    sig.ret = body.ty(expression).clone();
                }
                sig
            },
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
/// `identity(&xs[i])` would lose xs@items when its return tie is elided. With `in (a, b)`,
/// each input named whole.
fn result_roots(
    signature: &Signature,
    scope: &dz_check::ties::Scope<'_>,
) -> Vec<dz_check::ties::Root> {
    if !signature.sig.ret_ties.is_empty() {
        return signature
            .sig
            .ret_ties
            .iter()
            .filter_map(|path| super::normalize(scope, path).ok())
            .filter(|path| path.steps.is_empty())
            .map(|path| path.root)
            .collect();
    }
    let mut roots = Vec::new();
    if signature.sig.takes_self.is_some() {
        roots.push(dz_check::ties::Root::SelfValue);
    }
    // The same inputs `call_effects` elides over.
    for (i, param) in signature.sig.params.iter().enumerate() {
        if dz_check::ties::view_input(scope.table, &param.ty) {
            roots.push(dz_check::ties::Root::Param(i));
        }
    }
    if roots.len() == 1 {
        roots
    } else {
        Vec::new()
    }
}

/// Returned pointers into an indexed container get the same logical [*] target as
/// indexing, while retaining the allocation tie for address validity. This also works
/// through wrappers such as World::first. `path` is the `in` path `place` came from, none
/// when elided.
fn logical_result(
    signature: &Signature,
    scope: &dz_check::ties::Scope<'_>,
    bindings: &Bindings,
    path: Option<&TiePath>,
    place: &Place,
) -> Place {
    let Some(path) = path else { return place.clone() };
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
                // `index` returns a reference, which elides whatever its type.
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
