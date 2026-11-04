//! Where each allocation goes: a scope's region, the caller's result placement, or the heap
//! (spec §14.6, T-0117 stages 1 and 2, T-0119).
//!
//! A **site** is a call to a **result function**: one whose value owns memory and which
//! takes, out of sight, where to build it — a hidden last parameter, `_dz_result` in the C++
//! ([`result_functions`] says which take one). That is every constructor whose struct keeps
//! an allocator, `std`'s containers among them (`List<T>()`, `String(…)`, `HashMap`,
//! `HashSet`, `SlotMap`, `Path`, `Box`), which store it as `#placement`, and every function
//! returning such a value (`String::from`). Inside one, a site whose value is returned is
//! built there ([`Decision::Result`]); at a call, the caller passes its region, its own
//! result placement, or the heap, decided the same way for every site.
//!
//! Two more calls are sites that always keep the heap, so they are shown: `Rc(…)`, which a
//! `Weak` can outlive, and a call to the program's own function that takes an allocator
//! parameter left at `Allocators::Global`, since what it does with it is out of sight here.
//!
//! Each site is decided in two steps, per body and from nothing but that body and the
//! signatures it calls, so the editor, `dz placements` and codegen read one answer:
//!
//! 1. **The proposer** (the prototype's stage-1 rule, `dz_cpp/src/regions.rs` at the tag
//!    `regions-prototype`) proposes the region of block `B` for `let x = site` in `B` when
//!    every later use of `x` leaves it where it is: borrowed, indexed, sliced, measured,
//!    walked by a `for`, assigned to, a method's receiver, a field read, or copied.
//! 2. **The region check** walks the body's ties again with the proposals as facts
//!    ([`crate::ties::analyse_regions`]) and sends every placement it refuses to the heap,
//!    its finding as the reason. It shares no rule with the proposer: it is the safety net
//!    the prototype lacked, and a placement both accept is one the tie analysis can follow.
//!
//! Regions are per block, and a value is proposed for the block of the `let` that keeps it,
//! so a placed allocation runs once per entry of its block: the loop rule (T-0117), which
//! the check enforces.
//!
//! **A value assigned to a local** goes where the local lives, as a `let`'s does, and a
//! tuple `let` is followed as a `let` of each part (T-0146). On every pass of a loop the
//! local outlives, `s = String(…)` keeps the heap, since each pass drops the one before. The
//! exception is a local that grows from itself ([`crate::storing::grown_by`]): every
//! assignment holds what it replaces, `lhs = Binary{ lhs: Box(move(lhs)), … }`, so each
//! pass's nodes stay held, and a parser's tree is built in the region of its statement.
//!
//! **A value stored into a container** goes where the container lives (T-0145): an
//! argument a method stores into its receiver ([`crate::storing`]), when the receiver is a
//! local of the body, is proposed for that local's block, or for the result placement when
//! the container is returned. The loop rule has one exception there: a value built on every
//! pass and stored at once, by a method that keeps it, into a container that keeps
//! everything, may go into a region the loop does not end, since the region then grows no
//! faster than the container. The check follows the value into the container and back out:
//! an element taken out, or the container lent as `&mut`, carries it.
//!
//! A copy of a placed value carries no region: copying out of a region builds on the heap
//! (T-0117 step 3, `List::copy` and `HashMap::copy`). Both steps rely on that library rule.
//! `return x` is never a copy, since C++ moves a local it returns.
//!
//! The reasons are what the editor and `dz placements` print after `region: ` and `heap: `,
//! so they are worded here, short and naming the program's own code.

use std::collections::{HashMap, HashSet};

use dz_ast::Dispatch;
use dz_check::{ItemSig, TypeTable};
use dz_hir::ty::Ty;
use dz_hir::{self as hir, BlockId, Body, BodyOwner, Callee, ExprId, Hir, LocalId, StmtId};
use dz_project::ModuleId;
use dz_resolve::{Resolution, SymbolId};
use dz_span::{FileId, Span};

use crate::ties::{analyse_regions, Refusal, RegionRefusal};

/// Every body's sites, by the body's owner. A body with no site has no entry.
#[derive(Clone, Debug, Default)]
pub struct Placements {
    bodies: HashMap<BodyOwner, BodyPlacement>,
    /// The functions that take a hidden result placement ([`result_functions`]).
    results: HashSet<BodyOwner>,
    /// The by-value parameters that stay in the call ([`Staying`]).
    staying: Staying,
}

impl Placements {
    pub fn body(&self, owner: &BodyOwner) -> Option<&BodyPlacement> {
        self.bodies.get(owner)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&BodyOwner, &BodyPlacement)> {
        self.bodies.iter()
    }

    /// Whether the function `owner` takes a hidden result placement: its declaration has
    /// one more parameter than it writes, and every call passes one (stage 2).
    pub fn takes_result(&self, owner: &BodyOwner) -> bool {
        self.results.contains(owner)
    }

    /// The by-value parameters that stay in the call.
    pub fn staying(&self) -> &Staying {
        &self.staying
    }

    /// Record that `locals` of `owner`'s body are dropped by their region's release, and
    /// which of its sites built them ([`crate::release::skipped_drops`]).
    pub(crate) fn mark_skipped(
        &mut self,
        owner: &BodyOwner,
        locals: Vec<LocalId>,
        sites: &[ExprId],
    ) {
        let Some(body) = self.bodies.get_mut(owner) else { return };
        for s in &mut body.sites {
            if sites.contains(&s.call) {
                s.drop_skipped = true;
            }
        }
        body.skipped = locals;
    }
}

/// The function a call calls, as a body's owner: what [`Placements::takes_result`] and
/// [`Placements::body`] are asked about. `None` for a call through a function value.
pub fn callee_owner(res: &Resolution, callee: &Callee) -> Option<(SymbolId, BodyOwner)> {
    Some(match callee {
        Callee::Fn { sym, .. } => (*sym, BodyOwner::Fn(*sym)),
        Callee::Ctor { owner, index } => (*owner, BodyOwner::Ctor { owner: *owner, index: *index }),
        Callee::Assoc { owner, name, .. } | Callee::BaseMethod { owner, name, .. } => {
            (*owner, BodyOwner::Method { owner: *owner, name: name.clone() })
        }
        Callee::Method { owner, name, .. } => {
            let o = if res.symbol(*owner).kind == dz_resolve::SymbolKind::Trait {
                BodyOwner::TraitDefaults { owner: *owner, name: name.clone() }
            } else {
                BodyOwner::Method { owner: *owner, name: name.clone() }
            };
            (*owner, o)
        }
        Callee::Value(_) => return None,
    })
}

/// Whether a call is `std`'s `Box(value)`, whose first argument becomes the box's own
/// contents: it lives where the box lives, as a struct literal's fields live where the
/// struct does (spec §14.6, "a `Box`'s contents go where the `Box` goes").
pub(crate) fn boxes_argument(res: &Resolution, callee: &Callee) -> bool {
    match callee {
        Callee::Ctor { owner, .. } => {
            let s = res.symbol(*owner);
            s.name == "Box" && s.module == res.prelude
        }
        _ => false,
    }
}

/// Whether `parent` holds `e`'s value as its own, so `e`'s memory lives where `parent`'s
/// does: a `move`, an `or` (its operand or its fallback), a struct or union literal, a
/// tuple, an array, and the value a `Box` is made of. The proposer and the region check
/// climb the same chain.
pub(crate) fn holds_part(body: &Body, res: &Resolution, parent: ExprId, e: ExprId) -> bool {
    match &body.expr(parent).kind {
        hir::ExprKind::Move(_)
        | hir::ExprKind::StructLit { .. }
        | hir::ExprKind::UnionVariant { .. }
        | hir::ExprKind::Tuple(_)
        | hir::ExprKind::ArrayLit(_) => true,
        hir::ExprKind::Or { operand, clause } => {
            *operand == e || matches!(clause, hir::OrClause::Value(v) if *v == e)
        }
        hir::ExprKind::Call { callee, args } => {
            args.first() == Some(&e) && boxes_argument(res, callee)
        }
        _ => false,
    }
}

/// Whether `e` is a field of `self`, by fields alone: `m_items`, `self.a.b`. What a
/// constructor builds there lives where the value it makes is placed.
pub(crate) fn self_field(body: &Body, e: ExprId) -> bool {
    fn root(body: &Body, e: ExprId) -> bool {
        match body.expr(e).kind {
            hir::ExprKind::Local(l) => body.local(l).name == "self",
            hir::ExprKind::Deref(inner) => {
                matches!(body.expr(inner).kind, hir::ExprKind::Local(l) if body.local(l).name == "self")
            }
            hir::ExprKind::Field { base, .. } => root(body, base),
            _ => false,
        }
    }
    matches!(body.expr(e).kind, hir::ExprKind::Field { base, .. } if root(body, base))
}

/// When `e` is the text a `format!` builds — the first `let` of the macro's body, moved
/// out as its last statement (`dz_check`'s lowering) — the macro's own expression, whose
/// value the text is.
pub(crate) fn formatted_by(body: &Body, e: ExprId) -> Option<ExprId> {
    (0..body.exprs.len() as u32).map(ExprId).find(|f| {
        let hir::ExprKind::Formatted { body: block, .. } = body.expr(*f).kind else {
            return false;
        };
        let stmts = &body.block(block).stmts;
        let (Some(first), Some(last)) = (stmts.first(), stmts.last()) else { return false };
        let hir::StmtKind::Let { local: Some(text), init: Some(init) } = body.stmt(*first).kind
        else {
            return false;
        };
        let hir::StmtKind::Expr(tail) = body.stmt(*last).kind else { return false };
        let hir::ExprKind::Move(moved) = body.expr(tail).kind else { return false };
        init == e && matches!(body.expr(moved).kind, hir::ExprKind::Local(l) if l == text)
    })
}

/// The functions that take a hidden result placement (spec §14.6, T-0117 stage 2): each
/// whose result keeps an allocator ([`keeps_allocator`]), looking through `?`, decided
/// from signatures alone so that a declaration and every call agree:
///
/// - a free function, or a struct's or union's method that is neither `virtual` nor
///   `override`, that takes no allocator parameter of its own (a stage 1 site already says
///   where it builds);
/// - a trait's method, decided by the trait's signature, so every implementation (its
///   `ImplMethod` body, and the default `dz_mono` copies into it), the trait object's
///   vtable slot and a call through a bound all take one or none alike.
///
/// A function type takes one by the same rule ([`keeps_allocator`] on its result), so a
/// function value is called like a function; where the function it names takes none,
/// codegen adapts it. `virtual` methods are left out until a base and its overrides can be
/// decided together.
pub fn result_functions(hir: &Hir, table: &TypeTable, res: &Resolution) -> HashSet<BodyOwner> {
    let allocator = res.lookup(res.prelude, "Allocator");
    let holds = |t: &Ty| holds_allocator(table, allocator, t, &mut Vec::new());
    let mut out = HashSet::new();

    let mut traits: HashSet<SymbolId> =
        table.impls().iter().map(|i| i.trait_ref.trait_sym).collect();
    for (_, body) in hir.bodies() {
        if let BodyOwner::TraitDefaults { owner, .. } | BodyOwner::Method { owner, .. } =
            &body.owner
        {
            if res.symbol(*owner).kind == dz_resolve::SymbolKind::Trait {
                traits.insert(*owner);
            }
        }
    }
    for t in traits {
        let Some(s) = table.methods_sig(t) else { continue };
        for (name, sig) in &s.methods {
            if !holds(&sig.ret) {
                continue;
            }
            out.insert(BodyOwner::TraitDefaults { owner: t, name: name.clone() });
            out.insert(BodyOwner::Method { owner: t, name: name.clone() });
            for (idx, imp) in table.impls().iter().enumerate() {
                if imp.trait_ref.trait_sym == t {
                    out.insert(BodyOwner::ImplMethod { imp: idx as u32, name: name.clone() });
                }
            }
        }
    }

    for (_, body) in hir.bodies() {
        // A constructor builds into wherever its value is placed (round 3), when a field
        // keeps an allocator: it takes that placement and builds its fields there.
        if let BodyOwner::Ctor { owner, .. } = &body.owner {
            let fields = table.struct_sig(*owner).map(|s| s.fields.as_slice()).unwrap_or(&[]);
            if fields.iter().any(|f| holds(&f.ty)) && allocator_param(res, body).is_none() {
                out.insert(body.owner.clone());
            }
            continue;
        }
        // A `copy` builds into wherever the copy is placed, as a constructor does (T-0140).
        if let BodyOwner::Copy(owner) = &body.owner {
            if placed_copy_kind(table, res, *owner) == Some(PlacedCopy::Written) {
                out.insert(body.owner.clone());
            }
            continue;
        }
        let sig = match &body.owner {
            BodyOwner::Fn(sym) => match table.get(*sym) {
                Some(ItemSig::Fn(sig)) => sig,
                _ => continue,
            },
            BodyOwner::Method { owner, name }
                if res.symbol(*owner).kind != dz_resolve::SymbolKind::Trait =>
            {
                match table.methods_sig(*owner).and_then(|s| s.methods.get(name)) {
                    Some(sig) => sig,
                    None => continue,
                }
            }
            _ => continue,
        };
        if sig.dispatch != Dispatch::Static
            || !holds(&body.ret)
            || allocator_param(res, body).is_some()
        {
            continue;
        }
        out.insert(body.owner.clone());
    }
    out
}

/// How a struct's copy is built in a placement (T-0140), when it can be.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlacedCopy {
    /// Its own `copy`, which takes the placement as a result function does and reads it
    /// as `#placement`: `List`, `HashMap`.
    Written,
    /// Field by field, each field that keeps an allocator copied into the placement, the
    /// rest as C++ copies them: `String`, `Path`, `HashSet`, a program's own struct of them.
    Memberwise,
}

/// Whether struct `sym` has a copy that builds in a placement, and how (T-0140): one whose
/// fields keep an allocator, written by the program (no base, no `virtual` method, not a
/// `view struct`), with a `copy` of its own or neither `copy` nor `drop`. Its C++ has one
/// more constructor, `T(::dz::copy_tag, const T&, ^mut Allocator)`; the plain copy
/// constructor builds on the heap. Decided from the declaration, so codegen and every
/// instance agree; whether a given instance can be copied at all is [`placed_copy`]'s.
pub fn placed_copy_kind(table: &TypeTable, res: &Resolution, sym: SymbolId) -> Option<PlacedCopy> {
    let s = table.struct_sig(sym)?;
    if s.opaque
        || s.is_union
        || s.cpp_owned
        || s.polymorphic
        || s.base.is_some()
        || table.is_view_struct(sym)
    {
        return None;
    }
    let allocator = res.lookup(res.prelude, "Allocator");
    if !s.fields.iter().any(|f| holds_allocator(table, allocator, &f.ty, &mut Vec::new())) {
        return None;
    }
    match (&s.copy, &s.drop) {
        (Some(_), _) => Some(PlacedCopy::Written),
        (None, None) => Some(PlacedCopy::Memberwise),
        (None, Some(_)) => None,
    }
}

/// Whether a copy of `ty` can be built in a placement rather than on the heap (T-0140): it
/// is copyable, keeps an allocator, and its struct has a placed copy ([`placed_copy_kind`]);
/// built field by field, every field that keeps an allocator is placed too, so the whole
/// copy is where it is placed.
pub fn placed_copy(table: &TypeTable, res: &Resolution, ty: &Ty) -> bool {
    fn go(table: &TypeTable, res: &Resolution, ty: &Ty, seen: &mut Vec<Ty>) -> bool {
        let Ty::Named { sym, args } = ty else { return false };
        if seen.contains(ty) || !table.copyable(ty) || !keeps_allocator(table, res, ty) {
            return false;
        }
        seen.push(ty.clone());
        match placed_copy_kind(table, res, *sym) {
            Some(PlacedCopy::Written) => true,
            Some(PlacedCopy::Memberwise) => {
                let Some(s) = table.struct_sig(*sym) else { return false };
                table
                    .field_types(*sym, args, &s.fields)
                    .iter()
                    .all(|f| !keeps_allocator(table, res, f) || go(table, res, f, seen))
            }
            None => false,
        }
    }
    go(table, res, ty, &mut Vec::new())
}

/// The module a body belongs to.
pub fn owner_module(table: &TypeTable, res: &Resolution, owner: &BodyOwner) -> ModuleId {
    let sym = match owner {
        BodyOwner::Fn(s)
        | BodyOwner::Copy(s)
        | BodyOwner::Drop(s)
        | BodyOwner::Const(s)
        | BodyOwner::Static(s) => *s,
        BodyOwner::Method { owner, .. }
        | BodyOwner::Ctor { owner, .. }
        | BodyOwner::AssocConst { owner, .. }
        | BodyOwner::FieldDefault { owner, .. }
        | BodyOwner::TraitDefaults { owner, .. } => *owner,
        BodyOwner::ImplMethod { imp, .. } => return table.impls()[*imp as usize].module,
    };
    res.symbol(sym).module
}

/// The by-value parameters that stay in the call: the function only reads, borrows, copies
/// or drops what it is given there, or hands it to another such parameter, so the value is
/// gone when the call returns. A value passed to one may be in its caller's region, and
/// frees into it when the callee drops it: nothing changes in how the call is made.
///
/// Inferred from bodies, so only for a call within the callee's own module (T-0142), and
/// only for a call that reaches one known body: a free function, or a struct's or union's
/// method that is neither `virtual` nor `override`.
#[derive(Clone, Debug, Default)]
pub struct Staying {
    params: HashMap<BodyOwner, (ModuleId, Vec<usize>)>,
    /// The by-value parameters a method stores into `self` (T-0145): each with whether
    /// every call keeps it, never dropping it ([`crate::storing`]).
    stored: HashMap<BodyOwner, (ModuleId, Vec<(usize, bool)>)>,
}

impl Staying {
    /// Whether argument `arg` of a method call from a body in `module` is stored into the
    /// receiver, and if so whether every call keeps it (T-0145). Relied on within the
    /// callee's module, as [`Self::stays`] is, and for `std`'s containers from anywhere:
    /// `std` is fixed and analysed with every program.
    pub(crate) fn stores(
        &self,
        res: &Resolution,
        module: ModuleId,
        callee: &Callee,
        arg: usize,
    ) -> Option<bool> {
        if !matches!(callee, Callee::Method { .. }) {
            return None;
        }
        let (_, owner) = callee_owner(res, callee)?;
        let (m, params) = self.stored.get(&owner)?;
        if *m != module && *m != res.prelude {
            return None;
        }
        params.iter().find(|(i, _)| *i == arg).map(|(_, always)| *always)
    }

    /// Whether parameter `index` of `owner`, `self` not counted, is stored into `self`.
    pub fn param_stored(&self, owner: &BodyOwner, index: usize) -> bool {
        self.stored.get(owner).is_some_and(|(_, params)| params.iter().any(|(i, _)| *i == index))
    }

    /// Whether parameter `index` of `owner` is stored into `self` on every call, and never
    /// dropped instead.
    pub fn param_kept(&self, owner: &BodyOwner, index: usize) -> bool {
        self.stored.get(owner).is_some_and(|(_, params)| params.contains(&(index, true)))
    }

    /// Whether argument `arg` of a call from a body in `module` stays in the call.
    pub(crate) fn stays(
        &self,
        res: &Resolution,
        module: ModuleId,
        callee: &Callee,
        arg: usize,
    ) -> bool {
        let Some((_, owner)) = callee_owner(res, callee) else { return false };
        self.params.get(&owner).is_some_and(|(m, params)| *m == module && params.contains(&arg))
    }

    /// Whether parameter `index` of `owner`, `self` not counted, stays in the call.
    pub fn param_stays(&self, owner: &BodyOwner, index: usize) -> bool {
        self.params.get(owner).is_some_and(|(_, params)| params.contains(&index))
    }
}

/// Which by-value parameters stay in the call ([`Staying`]). Every parameter that keeps an
/// allocator is claimed, and a claim is withdrawn when the proposer sees the parameter leave
/// or the region check refuses it, the parameter checked as a region of the caller's that
/// the body may not return, store or pass on. Withdrawing one can withdraw others that hand
/// it on, so this repeats until nothing changes; claims only shrink, so it ends.
///
/// With [`Proposer::Everything`] only the region check withdraws a claim, so it can be
/// tested alone.
fn staying_params(hir: &Hir, table: &TypeTable, res: &Resolution, proposer: Proposer) -> Staying {
    let mut bodies: HashMap<BodyOwner, &Body> = HashMap::new();
    let mut claims: HashMap<BodyOwner, (ModuleId, Vec<usize>)> = HashMap::new();
    for (_, body) in hir.bodies() {
        let sig = match &body.owner {
            BodyOwner::Fn(sym) => match table.get(*sym) {
                Some(ItemSig::Fn(sig)) => sig,
                _ => continue,
            },
            BodyOwner::Method { owner, name }
                if res.symbol(*owner).kind != dz_resolve::SymbolKind::Trait =>
            {
                match table.methods_sig(*owner).and_then(|s| s.methods.get(name)) {
                    Some(sig) => sig,
                    None => continue,
                }
            }
            _ => continue,
        };
        if sig.dispatch != Dispatch::Static {
            continue;
        }
        let offset = usize::from(sig.takes_self.is_some());
        let params: Vec<usize> = (0..sig.params.len())
            .filter(|i| {
                body.params.get(i + offset).is_some_and(|l| {
                    let ty = &body.local(*l).ty;
                    !matches!(ty, Ty::Ref { .. } | Ty::Ptr { .. } | Ty::OwnPtr { .. })
                        && keeps_allocator(table, res, ty)
                })
            })
            .collect();
        if !params.is_empty() {
            claims.insert(body.owner.clone(), (owner_module(table, res, &body.owner), params));
            bodies.insert(body.owner.clone(), body);
        }
    }
    loop {
        let before = Staying { params: claims.clone(), ..Staying::default() };
        for (owner, (_, params)) in &mut claims {
            let body = bodies[owner];
            let offset = body.params.len() - owner_sig(table, owner).map_or(0, |s| s.params.len());
            if proposer == Proposer::Rules {
                let cx = Cx::new(table, res, body, false, &before);
                params
                    .retain(|i| matches!(cx.escape_of(body.params[i + offset], 0), Escape::Stays));
            }
            if params.is_empty() {
                continue;
            }
            let locals: Vec<LocalId> = params.iter().map(|i| body.params[i + offset]).collect();
            let refused = analyse_regions(body, table, res, &[], &locals, &before);
            let mut kept = refused.iter().map(Option::is_none);
            params.retain(|_| kept.next().unwrap_or(false));
        }
        claims.retain(|_, (_, params)| !params.is_empty());
        if claims.len() == before.params.len()
            && claims.iter().all(|(o, (_, p))| before.params.get(o).is_some_and(|(_, q)| p == q))
        {
            return before;
        }
    }
}

/// Whether a function returning `ty` takes a result placement, and a function type with
/// that result passes one: `ty` keeps an allocator, through its fields, `?` or `!`.
pub fn keeps_allocator(table: &TypeTable, res: &Resolution, ty: &Ty) -> bool {
    holds_allocator(table, res.lookup(res.prelude, "Allocator"), ty, &mut Vec::new())
}

/// A function's signature by its body's owner, `self` not among its parameters.
fn owner_sig<'t>(table: &'t TypeTable, owner: &BodyOwner) -> Option<&'t dz_check::FnSig> {
    match owner {
        BodyOwner::Fn(sym) => match table.get(*sym)? {
            ItemSig::Fn(sig) => Some(sig),
            _ => None,
        },
        BodyOwner::Method { owner, name } | BodyOwner::TraitDefaults { owner, name } => {
            table.methods_sig(*owner)?.methods.get(name)
        }
        BodyOwner::ImplMethod { imp, name } => table.impls().get(*imp as usize)?.methods.get(name),
        BodyOwner::Ctor { owner, index } => table.struct_sig(*owner)?.ctors.get(*index),
        _ => None,
    }
}

/// Whether a value of `ty` keeps an allocator, in a field or a field's field: what a
/// placement can be handed. `List`, `String`, `HashMap` and what holds them do; a
/// `Formatter` or a `File` needs a drop but allocates nothing. A type parameter is taken
/// not to: a function returning a bare `T` hands back a value that exists already (`pop`,
/// `get`, `take`), and whether it takes a placement must not depend on the instance, since
/// every instance is called the same way. `seen` stops a type that holds itself through a
/// pointer.
fn holds_allocator(
    table: &TypeTable,
    allocator: Option<SymbolId>,
    ty: &Ty,
    seen: &mut Vec<Ty>,
) -> bool {
    match ty {
        Ty::Ptr { inner, .. } | Ty::OwnPtr { inner, .. } => {
            matches!(&**inner, Ty::Named { sym, .. } if Some(*sym) == allocator)
        }
        Ty::Optional(inner) | Ty::Fallible(inner) => holds_allocator(table, allocator, inner, seen),
        Ty::Array { elem, .. } => holds_allocator(table, allocator, elem, seen),
        Ty::Tuple(parts) => parts.iter().any(|t| holds_allocator(table, allocator, t, seen)),
        Ty::Named { sym, args } => {
            if seen.contains(ty) {
                return false;
            }
            seen.push(ty.clone());
            let fields = match table.get(*sym) {
                Some(ItemSig::Struct(s)) => table.field_types(*sym, args, &s.fields),
                Some(ItemSig::Union { variants, .. }) => {
                    variants.iter().flat_map(|v| table.payload_types(*sym, args, v)).collect()
                }
                _ => Vec::new(),
            };
            fields.iter().any(|t| holds_allocator(table, allocator, t, seen))
        }
        _ => false,
    }
}

/// The parameter of `body` that defaults to `Allocators::Global`, by index, `self` not
/// counted.
fn allocator_param(res: &Resolution, body: &Body) -> Option<usize> {
    body.param_defaults.iter().position(|d| {
        d.is_some_and(|d| {
            matches!(
                &body.expr(d).kind,
                hir::ExprKind::AssocConst { owner, name }
                    if name == "Global" && res.symbol(*owner).name == "Allocators"
            )
        })
    })
}

/// One body's sites, in source order.
#[derive(Clone, Debug)]
pub struct BodyPlacement {
    pub file: FileId,
    pub sites: Vec<Site>,
    /// The sites the proposer placed and the region check sent to the heap.
    demoted: Vec<ExprId>,
    /// The locals whose drop is left to their region's release (T-0118).
    skipped: Vec<LocalId>,
}

impl BodyPlacement {
    /// The blocks that declare a region, each once, in id order.
    pub fn region_blocks(&self) -> Vec<BlockId> {
        let mut blocks: Vec<BlockId> = self
            .sites
            .iter()
            .filter_map(|s| match s.decision {
                Decision::Region { block, .. } => Some(block),
                Decision::Result { .. } | Decision::Heap { .. } => None,
            })
            .collect();
        blocks.sort_by_key(|b| b.0);
        blocks.dedup();
        blocks
    }

    /// For a call placed in a region: where its placement is passed in the callee's
    /// parameters (not counting `self`), and the region's block.
    pub fn region_for(&self, call: ExprId) -> Option<(usize, BlockId)> {
        match self.placed(call)? {
            (arg, Placed::Region(block)) => Some((arg, block)),
            (_, Placed::Result) => None,
        }
    }

    /// For a call placed anywhere but the heap: where its placement is passed in the
    /// callee's parameters (not counting `self`; for a result function, the hidden one
    /// past the last it writes), and where the memory comes from.
    pub fn placed(&self, call: ExprId) -> Option<(usize, Placed)> {
        self.sites.iter().find(|s| s.call == call).and_then(|s| match s.decision {
            Decision::Region { block, .. } => Some((s.arg, Placed::Region(block))),
            Decision::Result { .. } => Some((s.arg, Placed::Result)),
            Decision::Heap { .. } => None,
        })
    }

    /// The calls the proposer placed and the region check refused.
    pub fn demoted(&self) -> &[ExprId] {
        &self.demoted
    }

    /// Whether `local`'s drop is skipped: everything it owns is in its block's region,
    /// which frees it all at once, and its drop would do nothing but free (T-0118).
    pub fn skips_drop(&self, local: LocalId) -> bool {
        self.skipped.contains(&local)
    }
}

/// One call that allocates, placed by the compiler.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Site {
    pub call: ExprId,
    pub span: Span,
    /// The callee as a reader names it: `List<Node>`, `String::from`.
    pub callee: String,
    /// Where the placement is passed in the callee's parameters, not counting `self`: for
    /// a result function, the hidden one past the last it writes.
    pub arg: usize,
    pub decision: Decision,
    /// The value is dropped by its region's release, not by its drop (T-0118).
    pub drop_skipped: bool,
}

impl Site {
    /// What the editor's hint and `dz placements` print: the decision, and whether the drop
    /// of what it built is skipped, "region: `main`; drop skipped".
    pub fn label(&self) -> String {
        let label = self.decision.label();
        if self.drop_skipped {
            format!("{label}; drop skipped")
        } else {
            label
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decision {
    /// In the region of `block`, released when it ends. `scope` says which, for a reader:
    /// "`update`", "each pass of a loop in `main`".
    Region { block: BlockId, scope: String },
    /// Returned, so built wherever the function's caller keeps its result: the body's
    /// hidden result placement. `function` names the function, for a reader.
    Result { function: String },
    /// On the heap, and why; `at` is the use that decided it, where there is one.
    Heap { why: String, at: Option<Span> },
}

impl Decision {
    /// What the editor's hint and `dz placements` print: "region: `update`", "result:
    /// where `evens`' caller keeps it", "heap: stored in `self.items`". One spelling, so
    /// the two cannot disagree.
    pub fn label(&self) -> String {
        match self {
            Decision::Region { scope, .. } => format!("region: {scope}"),
            Decision::Result { function } => format!("result: where {function}'s caller keeps it"),
            Decision::Heap { why, .. } => format!("heap: {why}"),
        }
    }
}

/// Where a placed call's memory comes from, for codegen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placed {
    /// The region of this block.
    Region(BlockId),
    /// The body's own hidden result placement.
    Result,
}

/// Which proposals the region check is given.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Proposer {
    /// The proposer's rule: what [`place`] does.
    Rules,
    /// Every site in `std`'s placeable calls, in the region of the innermost block around
    /// it, so the region check alone decides. For testing the check: the fixed rules (`Rc`,
    /// the program's own functions, a formatting macro's body) still apply.
    Everything,
}

/// Decide every site in the program, body by body.
pub fn place(hir: &Hir, table: &TypeTable, res: &Resolution) -> Placements {
    place_with(hir, table, res, Proposer::Rules)
}

/// [`place`], with the proposals made by `proposer`.
pub fn place_with(
    hir: &Hir,
    table: &TypeTable,
    res: &Resolution,
    proposer: Proposer,
) -> Placements {
    let results = result_functions(hir, table, res);
    let mut staying = staying_params(hir, table, res, proposer);
    staying.stored = crate::storing::storing_params(hir, table, res, &staying);
    let mut out = Placements::default();
    for (_, body) in hir.bodies() {
        if let Some(placed) = place_body(hir, table, res, &results, &staying, body, proposer) {
            out.bodies.insert(body.owner.clone(), placed);
        }
    }
    out.results = results;
    out.staying = staying;
    crate::release::skipped_drops(hir, table, res, &mut out);
    out
}

/// Where a site is proposed to go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    /// The region of this block.
    Block(BlockId),
    /// The body's own result placement: the value is returned.
    Result,
}

/// Where a site is proposed to go, or why it keeps the heap and where that was decided.
type Proposal = Result<Target, (String, Option<Span>)>;

/// What a site's callee is, as far as placement cares.
struct Callable {
    /// Where the placement is passed, not counting `self`: a result function's hidden
    /// parameter, past the last it writes, or an allocator parameter's index.
    arg: usize,
    /// Declared in `std`.
    std: bool,
    /// A result function: its body builds only its result in the placement it is given,
    /// which the check holds it to, so the call can be placed whoever wrote it.
    result: bool,
    /// `Rc`, which a `Weak` can outlive (spec §14.6).
    rc: bool,
    name: String,
}

fn place_body(
    hir: &Hir,
    table: &TypeTable,
    res: &Resolution,
    results: &HashSet<BodyOwner>,
    staying: &Staying,
    body: &Body,
    proposer: Proposer,
) -> Option<BodyPlacement> {
    let mut found = Vec::new();
    let mut copies = Vec::new();
    for i in 0..body.exprs.len() {
        let e = ExprId(i as u32);
        if let Some(c) = callable(hir, table, res, results, body, e) {
            found.push((e, c));
        } else if copied(table, res, body, e) {
            copies.push(e);
        }
    }
    if found.is_empty() && copies.is_empty() {
        return None;
    }
    let cx = Cx::new(table, res, body, results.contains(&body.owner), staying);
    // A copy is a site when it is taken by value somewhere a placement can follow it
    // (T-0140): what it builds is its own, wherever the value it copies lives.
    for e in copies {
        if cx.copied_by_value(e) {
            let name = format!("a copy of `{}`", place_name(res, body, Some(table), e));
            found.push((e, Callable { arg: 0, std: true, result: true, rc: false, name }));
        }
    }
    if found.is_empty() {
        return None;
    }
    found.sort_by_key(|(e, _)| (body.expr(*e).span.start, e.0));

    // Step 1: what is proposed, and why the rest is not.
    let mut decided: Vec<(ExprId, Callable, Proposal)> = Vec::new();
    for (e, c) in found {
        let decision = cx.propose(e, &c, proposer);
        decided.push((e, c, decision));
    }

    // Step 2: the region check, over every proposal at once. A result placement outlives
    // the body: it is checked as a region of the body's outermost block that its end
    // does not release, and that a `return` may take.
    let proposals: Vec<(ExprId, BlockId, bool)> = decided
        .iter()
        .filter_map(|(e, _, d)| match d {
            Ok(Target::Block(b)) => Some((*e, *b, false)),
            Ok(Target::Result) => Some((*e, body.root, true)),
            Err(_) => None,
        })
        .collect();
    let refusals = if proposals.is_empty() {
        Vec::new()
    } else {
        analyse_regions(body, table, res, &proposals, &[], staying)
    };
    let refused: HashMap<ExprId, RegionRefusal> =
        proposals.iter().zip(refusals).filter_map(|((e, _, _), r)| r.map(|r| (*e, r))).collect();

    let mut sites = Vec::new();
    let mut demoted = Vec::new();
    for (e, c, d) in decided {
        let decision = match d {
            Ok(target) => match refused.get(&e) {
                Some(r) => {
                    demoted.push(e);
                    Decision::Heap { why: cx.refusal(&r.why), at: Some(r.at) }
                }
                None => match target {
                    Target::Block(block) => Decision::Region { block, scope: cx.scope(block) },
                    Target::Result => Decision::Result { function: cx.function() },
                },
            },
            Err((why, at)) => Decision::Heap { why, at },
        };
        let span = body.expr(e).span;
        sites.push(Site {
            call: e,
            span,
            callee: c.name,
            arg: c.arg,
            decision,
            drop_skipped: false,
        });
    }
    Some(BodyPlacement { file: body.file, sites, demoted, skipped: Vec::new() })
}

/// `e`, when it is a site: a call to a result function, `std`'s `Rc(…)`, or a call whose
/// callee has a parameter defaulting to `Allocators::Global` that the call leaves out.
fn callable(
    hir: &Hir,
    table: &TypeTable,
    res: &Resolution,
    results: &HashSet<BodyOwner>,
    body: &Body,
    e: ExprId,
) -> Option<Callable> {
    let node = body.expr(e);
    let hir::ExprKind::Call { callee, args } = &node.kind else { return None };
    let name = callee_name(res, body, callee, &node.ty);
    // A function value passes a placement when its type's result keeps an allocator: what
    // it calls is out of sight, but whatever it is takes one, adapted if need be.
    if let Callee::Value(v) = callee {
        let Ty::Fn { params, ret, .. } = body.ty(*v).peeled() else { return None };
        if !keeps_allocator(table, res, ret) {
            return None;
        }
        return Some(Callable { arg: params.len(), std: false, result: true, rc: false, name });
    }
    let (sym, owner) = callee_owner(res, callee)?;
    let s = res.symbol(sym);
    let std = s.module == res.prelude;
    if results.contains(&owner) {
        let arg = owner_sig(table, &owner).map(|sig| sig.params.len())?;
        return Some(Callable { arg, std, result: true, rc: false, name });
    }
    // `Rc(…)` takes no placement, since its value is always the heap's, but it allocates,
    // so it is shown as a site that keeps the heap.
    if std && s.name == "Rc" && matches!(callee, Callee::Ctor { .. }) {
        let arg = owner_sig(table, &owner).map(|sig| sig.params.len())?;
        return Some(Callable { arg, std, result: false, rc: true, name });
    }
    let callee_body = hir.find(&owner)?;
    let arg = allocator_param(res, callee_body)?;
    let left_out = match args.get(arg) {
        None => true,
        Some(a) => matches!(body.expr(*a).kind, hir::ExprKind::DefaultArg(_)),
    };
    if !left_out {
        return None;
    }
    Some(Callable { arg, std, result: false, rc: false, name })
}

/// Whether `e` reads a place by value without moving it, and the copy that makes can be
/// built in a placement ([`placed_copy`]).
fn copied(table: &TypeTable, res: &Resolution, body: &Body, e: ExprId) -> bool {
    matches!(
        body.expr(e).kind,
        hir::ExprKind::Local(_)
            | hir::ExprKind::Field { .. }
            | hir::ExprKind::TupleIndex { .. }
            | hir::ExprKind::Index { .. }
            | hir::ExprKind::Deref(_)
    ) && placed_copy(table, res, body.ty(e))
}

/// A callee as a reader names it.
fn callee_name(res: &Resolution, body: &Body, callee: &Callee, ty: &Ty) -> String {
    match callee {
        Callee::Fn { sym, .. } => res.symbol(*sym).name.clone(),
        Callee::Ctor { .. } => dz_check::type_name(res, ty),
        Callee::Assoc { owner, type_args, name, .. } => {
            let owner = Ty::Named { sym: *owner, args: type_args.clone() };
            format!("{}::{name}", dz_check::type_name(res, &owner))
        }
        Callee::Method { recv, name, .. } | Callee::BaseMethod { recv, name, .. } => {
            format!("{}.{name}", place_name(res, body, None, *recv))
        }
        Callee::Value(_) => "a function value".to_owned(),
    }
}

/// A place as the program writes it: `self.items`, `names[…]`.
fn place_name(res: &Resolution, body: &Body, table: Option<&TypeTable>, e: ExprId) -> String {
    match &body.expr(e).kind {
        hir::ExprKind::Local(l) => body.local(*l).name.clone(),
        hir::ExprKind::Field { base, index } => {
            let field = table.and_then(|t| match body.ty(*base).peeled_refs() {
                Ty::Named { sym, .. } => {
                    t.struct_sig(*sym).and_then(|s| s.fields.get(*index)).map(|f| f.name.clone())
                }
                _ => None,
            });
            let field = field.unwrap_or_else(|| index.to_string());
            format!("{}.{field}", place_name(res, body, table, *base))
        }
        hir::ExprKind::TupleIndex { base, index } => {
            format!("{}.{index}", place_name(res, body, table, *base))
        }
        hir::ExprKind::Index { base, .. } => format!("{}[…]", place_name(res, body, table, *base)),
        hir::ExprKind::Deref(inner) => place_name(res, body, table, *inner),
        hir::ExprKind::Static(sym) => res.symbol(*sym).name.clone(),
        // `xs[i]` on a container is a call to its `Index` or `IndexMut` (spec §11).
        hir::ExprKind::Call { callee: Callee::Method { recv, name, .. }, .. }
            if name == "index" || name == "index_mut" =>
        {
            format!("{}[…]", place_name(res, body, table, *recv))
        }
        hir::ExprKind::Call { callee: Callee::Method { recv, name, .. }, .. } => {
            format!("{}.{name}()", place_name(res, body, table, *recv))
        }
        _ => "…".to_owned(),
    }
}

/// How a block is entered, for naming its region.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Root,
    Loop,
    If,
    Case,
    Or,
    Plain,
    /// A formatting macro's body, which C++ gets as a lambda: no region is declared there.
    Lambda,
}

/// What is around a node.
#[derive(Clone, Copy)]
enum Up {
    Expr(ExprId),
    Stmt(StmtId),
}

struct Cx<'a> {
    table: &'a TypeTable,
    res: &'a Resolution,
    body: &'a Body,
    up: HashMap<ExprId, Up>,
    expr_block: HashMap<ExprId, BlockId>,
    stmt_block: HashMap<StmtId, BlockId>,
    /// Each block's kind, the block around it, and the expression it is part of.
    blocks: HashMap<BlockId, (Kind, Option<BlockId>, Option<ExprId>)>,
    /// The body is a result function's: what it returns can be built where its caller
    /// keeps it.
    returns_result: bool,
    staying: &'a Staying,
    /// The body's module, which a call must share with its callee to rely on [`Staying`].
    module: ModuleId,
}

/// What the uses of a local do with it.
enum Escape {
    /// It stays where it is: borrowed, copied, assigned to.
    Stays,
    /// It is returned, and stays otherwise.
    Returned,
    /// It is stored into the container in local `container`, by a method that stores the
    /// argument (T-0145), and stays otherwise. `direct` when it is the argument itself or a
    /// part of it, never held by a `let` on the way; `kept` when the method keeps it on
    /// every call.
    Into { container: LocalId, direct: bool, kept: bool },
    /// It is assigned to the whole of a local the body declares, which stays (T-0146): it
    /// lives where that local does.
    Held(LocalId),
    /// It leaves, and why and where.
    Leaves((String, Option<Span>)),
}

impl<'a> Cx<'a> {
    fn new(
        table: &'a TypeTable,
        res: &'a Resolution,
        body: &'a Body,
        returns_result: bool,
        staying: &'a Staying,
    ) -> Self {
        let mut cx = Cx {
            table,
            res,
            body,
            returns_result,
            staying,
            module: owner_module(table, res, &body.owner),
            up: HashMap::new(),
            expr_block: HashMap::new(),
            stmt_block: HashMap::new(),
            blocks: HashMap::new(),
        };
        cx.walk_block(body.root, Kind::Root, None, None);
        cx
    }

    fn walk_block(&mut self, b: BlockId, kind: Kind, parent: Option<BlockId>, by: Option<ExprId>) {
        self.blocks.insert(b, (kind, parent, by));
        for s in self.body.block(b).stmts.clone() {
            self.walk_stmt(s, b);
        }
    }

    fn walk_stmt(&mut self, s: StmtId, b: BlockId) {
        self.stmt_block.insert(s, b);
        let up = Up::Stmt(s);
        match &self.body.stmt(s).kind {
            hir::StmtKind::Let { init, .. } => {
                if let Some(e) = init {
                    self.walk_expr(*e, up, b);
                }
            }
            hir::StmtKind::LetTuple { init, .. } => self.walk_expr(*init, up, b),
            hir::StmtKind::Assign { target, value, .. } => {
                self.walk_expr(*value, up, b);
                self.walk_expr(*target, up, b);
            }
            hir::StmtKind::Expr(e) => self.walk_expr(*e, up, b),
            hir::StmtKind::While { cond, body, .. } => {
                self.walk_expr(*cond, up, b);
                self.walk_block(*body, Kind::Loop, Some(b), None);
            }
            hir::StmtKind::For { subject, body, .. } => {
                self.walk_expr(*subject, up, b);
                self.walk_block(*body, Kind::Loop, Some(b), None);
            }
            hir::StmtKind::Defer(inner) => self.walk_stmt(*inner, b),
            hir::StmtKind::Item(_) | hir::StmtKind::Error => {}
        }
    }

    fn walk_expr(&mut self, e: ExprId, up: Up, b: BlockId) {
        self.up.insert(e, up);
        self.expr_block.insert(e, b);
        let here = Up::Expr(e);
        let kind = self.body.expr(e).kind.clone();
        let sub = |cx: &mut Self, x: ExprId| cx.walk_expr(x, here, b);
        match kind {
            hir::ExprKind::Field { base, .. }
            | hir::ExprKind::TupleIndex { base, .. }
            | hir::ExprKind::Len(base)
            | hir::ExprKind::DataPtr(base)
            | hir::ExprKind::Deref(base)
            | hir::ExprKind::Move(base)
            | hir::ExprKind::Ref { place: base, .. }
            | hir::ExprKind::Unary { operand: base, .. }
            | hir::ExprKind::Cast { operand: base, .. } => sub(self, base),
            hir::ExprKind::Index { base, index } => {
                sub(self, base);
                sub(self, index);
            }
            hir::ExprKind::SliceRange { base, start, end } => {
                sub(self, base);
                for x in [start, end].into_iter().flatten() {
                    sub(self, x);
                }
            }
            hir::ExprKind::Binary { lhs, rhs, .. } => {
                sub(self, lhs);
                sub(self, rhs);
            }
            hir::ExprKind::Range { start, end, .. } => {
                for x in [start, end].into_iter().flatten() {
                    sub(self, x);
                }
            }
            hir::ExprKind::StructLit { fields, .. }
            | hir::ExprKind::UnionVariant { payload: fields, .. } => {
                for x in fields.into_iter().flatten() {
                    sub(self, x);
                }
            }
            hir::ExprKind::Tuple(parts)
            | hir::ExprKind::ArrayLit(parts)
            | hir::ExprKind::Builtin { args: parts, .. }
            | hir::ExprKind::Intrinsic { args: parts, .. } => {
                for x in parts {
                    sub(self, x);
                }
            }
            hir::ExprKind::Call { callee, args } => {
                match callee {
                    Callee::Method { recv, .. } | Callee::BaseMethod { recv, .. } => {
                        sub(self, recv)
                    }
                    Callee::Value(v) => sub(self, v),
                    _ => {}
                }
                for x in args {
                    sub(self, x);
                }
            }
            hir::ExprKind::Block(block) => self.walk_block(block, Kind::Plain, Some(b), Some(e)),
            hir::ExprKind::If { cond, then, else_branch } => {
                sub(self, cond);
                self.walk_block(then, Kind::If, Some(b), Some(e));
                if let Some(other) = else_branch {
                    self.walk_block(other, Kind::If, Some(b), Some(e));
                }
            }
            hir::ExprKind::IfLet { operand, then, else_branch, .. } => {
                sub(self, operand);
                self.walk_block(then, Kind::If, Some(b), Some(e));
                if let Some(other) = else_branch {
                    self.walk_block(other, Kind::If, Some(b), Some(e));
                }
            }
            hir::ExprKind::Switch { subject, arms, default } => {
                sub(self, subject);
                for arm in arms {
                    for v in arm.values {
                        sub(self, v);
                    }
                    self.walk_block(arm.body, Kind::Case, Some(b), Some(e));
                }
                if let Some(d) = default {
                    self.walk_block(d, Kind::Case, Some(b), Some(e));
                }
            }
            hir::ExprKind::Guarded { init, value, .. } => {
                sub(self, init);
                sub(self, value);
            }
            hir::ExprKind::Formatted { args, body, .. } => {
                for x in args {
                    sub(self, x);
                }
                self.walk_block(body, Kind::Lambda, Some(b), Some(e));
            }
            hir::ExprKind::Or { operand, clause } => {
                sub(self, operand);
                match clause {
                    hir::OrClause::Value(v) => sub(self, v),
                    hir::OrClause::Block { body, .. } => {
                        self.walk_block(body, Kind::Or, Some(b), Some(e))
                    }
                    hir::OrClause::Ignore | hir::OrClause::Unreachable => {}
                }
            }
            hir::ExprKind::Return(Some(v)) => sub(self, v),
            _ => {}
        }
    }

    // --- the proposer -----------------------------------------------------------------

    /// Where `e` is proposed to go — a block's region, or the body's result placement when
    /// it is returned — or why it keeps the heap.
    fn propose(&self, e: ExprId, c: &Callable, proposer: Proposer) -> Proposal {
        if !c.std && !c.result {
            return Err((format!("made inside `{}`", c.name), None));
        }
        if c.rc {
            return Err(("a `Weak` can outlive the scope".to_owned(), None));
        }
        let what = match &self.body.owner {
            BodyOwner::Const(_) | BodyOwner::AssocConst { .. } => Some("a constant's value"),
            BodyOwner::Static(_) => Some("a static's value"),
            BodyOwner::FieldDefault { .. } => Some("a field's default"),
            _ => None,
        };
        if let Some(what) = what {
            return Err((what.to_owned(), None));
        }
        let Some(&block) = self.expr_block.get(&e) else {
            // A parameter's default or a base constructor's argument: evaluated by a
            // caller, or before the body.
            return Err(("not inside the body's blocks".to_owned(), None));
        };
        // `format!`'s text is the macro's value, so it goes where that value goes. The
        // lambda C++ gets captures by reference, so it reaches the region outside it.
        let formatted = self.formatted_text(e, block);
        let (e, block) = formatted.unwrap_or((e, block));
        if self.blocks.get(&block).is_some_and(|(k, _, _)| *k == Kind::Lambda) {
            return Err(("inside a formatting macro".to_owned(), None));
        }
        if proposer == Proposer::Everything {
            return Ok(Target::Block(block));
        }
        // What holds the site's value — `make() or …`, `Store{ names: List() }`,
        // `Box(Node{ … })` — places it: a value's owned parts live where it lives.
        let top = self.holder(e);
        match self.up.get(&top) {
            Some(Up::Stmt(s)) => match &self.body.stmt(*s).kind {
                hir::StmtKind::Let { local: Some(x), init: Some(init) } if *init == top => {
                    let local = self.body.local(*x);
                    if local.ty != *self.body.ty(top) {
                        let ty = dz_check::type_name(self.res, &local.ty);
                        return Err((format!("kept as `{ty}`"), Some(local.span)));
                    }
                    let block = self.stmt_block.get(s).copied().unwrap_or(block);
                    self.kept_in(self.escape_of(*x, 0), block, top)
                }
                hir::StmtKind::Let { .. } => Err(("not kept by `let`".to_owned(), None)),
                // `let (name, tree) = parser.statement() or …`: each part is a `let` of its
                // own, and the tuple goes where they all do (T-0146).
                hir::StmtKind::LetTuple { locals, init } if *init == top => {
                    let block = self.stmt_block.get(s).copied().unwrap_or(block);
                    let locals: Vec<LocalId> = locals.iter().flatten().copied().collect();
                    self.kept_in(self.escape_of_all(&locals, 0), block, top)
                }
                hir::StmtKind::Assign { target, value, op, .. } if *value == top => {
                    // A constructor or a `copy` building a field of the value it makes:
                    // where that value is placed (round 3, T-0140).
                    let ctor =
                        matches!(self.body.owner, BodyOwner::Ctor { .. } | BodyOwner::Copy(_));
                    if ctor
                        && self.returns_result
                        && *op == hir::AssignOp::Assign
                        && self_field(self.body, *target)
                    {
                        return Ok(Target::Result);
                    }
                    let name = place_name(self.res, self.body, Some(self.table), *target);
                    // A whole local of the body: the value lives where the local does, as
                    // a `let`'s would. On every pass of a loop the local outlives only when
                    // the local grows from itself, so nothing a pass makes dies (T-0146).
                    if let (Some(y), hir::AssignOp::Assign) = (self.assigned_local(*target), op) {
                        let outer = self.declared_in(y).unwrap_or(self.body.root);
                        let grows = crate::storing::grown_by(self.body, self.table, self.res, top)
                            == Some(y);
                        if grows || self.loop_between(block, outer) == Some(false) {
                            return match self.escape_of(y, 1) {
                                Escape::Stays => Ok(Target::Block(outer)),
                                other => self.kept_in(other, block, top),
                            };
                        }
                    }
                    let why = if self.in_loop(block) {
                        format!("assigned to `{name}` on every pass of a loop")
                    } else {
                        format!("assigned to `{name}`; only a `let` gets a region")
                    };
                    Err((why, Some(self.body.stmt(*s).span)))
                }
                _ => self.temporary(top, block),
            },
            _ => self.temporary(top, block),
        }
    }

    /// Whether the copy `e` makes is taken by value where the proposer and the region
    /// check both follow a value (T-0140): a `let`'s value, an argument, a part of a struct
    /// or union literal, a tuple or an array, what is returned (a local returned is moved,
    /// never copied), or, in a `copy`, what is assigned to a field of `self`.
    fn copied_by_value(&self, e: ExprId) -> bool {
        match self.up.get(&e) {
            Some(Up::Stmt(s)) => match self.body.stmt(*s).kind {
                hir::StmtKind::Let { init: Some(init), .. } => init == e,
                hir::StmtKind::Assign { target, value, op: hir::AssignOp::Assign, .. } => {
                    value == e
                        && matches!(self.body.owner, BodyOwner::Copy(_))
                        && self_field(self.body, target)
                }
                _ => false,
            },
            Some(Up::Expr(p)) => match &self.body.expr(*p).kind {
                hir::ExprKind::StructLit { .. }
                | hir::ExprKind::UnionVariant { .. }
                | hir::ExprKind::Tuple(_)
                | hir::ExprKind::ArrayLit(_) => true,
                hir::ExprKind::Call { args, .. } => args.contains(&e),
                hir::ExprKind::Return(Some(_)) => {
                    !matches!(self.body.expr(e).kind, hir::ExprKind::Local(_))
                }
                _ => false,
            },
            None => false,
        }
    }

    /// When `e` is the text a `format!` builds, the macro's own expression and the block it
    /// sits in, which place the text as they would any value ([`formatted_by`]).
    fn formatted_text(&self, e: ExprId, block: BlockId) -> Option<(ExprId, BlockId)> {
        let (Kind::Lambda, _, Some(by)) = *self.blocks.get(&block)? else { return None };
        (formatted_by(self.body, e)? == by).then_some((by, *self.expr_block.get(&by)?))
    }

    /// A value no `let` keeps: placed when it is what the body returns, or what a method
    /// stores into a container (T-0145). `block` is where it is made. `format!`'s text
    /// comes here as the macro's value, which is the text itself, moved out of the macro:
    /// stored at once, it is stored directly (T-0149).
    fn temporary(&self, e: ExprId, block: BlockId) -> Proposal {
        // The value `e` makes, a copy site's included.
        match self.fate_as(e, 0, false) {
            Escape::Returned => Ok(Target::Result),
            Escape::Into { container, direct, kept } => {
                self.stored_target(container, block, direct, kept)
            }
            Escape::Held(y) => self.held_target(y, block, e),
            Escape::Leaves(found) => Err(found),
            Escape::Stays => Err(("a temporary".to_owned(), None)),
        }
    }

    /// Where a value made in `block` and kept by a `let` goes, given what becomes of it:
    /// `block`'s region when it stays there, the result placement when it is returned, a
    /// container's or another local's place when it moves into one. `top` is the value.
    fn kept_in(&self, escape: Escape, block: BlockId, top: ExprId) -> Proposal {
        match escape {
            Escape::Stays => Ok(Target::Block(block)),
            Escape::Returned => Ok(Target::Result),
            // Held by a `let` first, so not stored directly.
            Escape::Into { container, kept, .. } => {
                self.stored_target(container, block, false, kept)
            }
            Escape::Held(y) => self.held_target(y, block, top),
            Escape::Leaves(found) => Err(found),
        }
    }

    /// Where a value made in `block` goes when it ends up assigned to the local `y`
    /// (T-0146): the region of `y`'s block, or of `block` when that one is further out. On
    /// every pass of a loop `y` outlives, only when `y` grows from itself, holding each
    /// pass's value in its next ([`crate::storing::grown_by`]); otherwise the value it
    /// replaces would die with the region still growing.
    fn held_target(&self, y: LocalId, block: BlockId, top: ExprId) -> Proposal {
        let name = self.body.local(y).name.clone();
        let Some(at) = self.declared_in(y) else {
            return Err((format!("stored in `{name}`, which is not a `let` of this body"), None));
        };
        let region = if self.loop_between(block, at).is_some() {
            at
        } else if self.loop_between(at, block).is_some() {
            block
        } else {
            return Err((format!("moved to `{name}`"), None));
        };
        if self.loop_between(block, region) == Some(true)
            && crate::storing::grown_by(self.body, self.table, self.res, top) != Some(y)
        {
            return Err((format!("moved to `{name}` on every pass of a loop"), None));
        }
        Ok(Target::Block(region))
    }

    fn in_loop(&self, mut block: BlockId) -> bool {
        loop {
            match self.blocks.get(&block) {
                Some((Kind::Loop, _, _)) => return true,
                Some((_, Some(parent), _)) => block = *parent,
                _ => return false,
            }
        }
    }

    /// What the uses of `x` do with it: the first that takes it from where it is, in source
    /// order; else whether one returns it. `depth` bounds following one local into the
    /// next.
    fn escape_of(&self, x: LocalId, depth: u32) -> Escape {
        self.escape_of_all(&[x], depth)
    }

    /// [`Self::escape_of`] for the locals a tuple `let` binds, which together hold the
    /// tuple: where the first that leaves takes it, or else where they all go.
    fn escape_of_all(&self, xs: &[LocalId], depth: u32) -> Escape {
        let mut uses: Vec<ExprId> = (0..self.body.exprs.len() as u32)
            .map(ExprId)
            .filter(
                |e| matches!(self.body.expr(*e).kind, hir::ExprKind::Local(l) if xs.contains(&l)),
            )
            .collect();
        uses.sort_by_key(|e| (self.body.expr(*e).span.start, e.0));
        let mut returned = false;
        let mut into = None;
        for u in uses {
            // Moved into the local's own next value, `lhs = Binary{ lhs: Box(move(lhs)), … }`:
            // it stays where the local is (T-0146).
            if self.held_by_itself(u) {
                continue;
            }
            match self.fate(u, depth) {
                Escape::Stays => {}
                Escape::Returned => returned = true,
                Escape::Into { container, direct, kept } => match into {
                    // Stored into two containers, or into one twice: on different paths, or
                    // one a copy. Neither is followed.
                    Some(_) => {
                        let span = Some(self.body.expr(u).span);
                        return Escape::Leaves(("stored in two places".to_owned(), span));
                    }
                    None => into = Some(Escape::Into { container, direct, kept }),
                },
                Escape::Held(y) => match into {
                    Some(Escape::Held(z)) if z == y => {}
                    Some(_) => {
                        let span = Some(self.body.expr(u).span);
                        return Escape::Leaves(("stored in two places".to_owned(), span));
                    }
                    None => into = Some(Escape::Held(y)),
                },
                Escape::Leaves(found) => return Escape::Leaves(found),
            }
        }
        match (returned, into) {
            // Kept by a local on one path and returned on another: the result placement
            // outlives the local.
            (true, Some(Escape::Held(_))) => Escape::Returned,
            (true, Some(_)) => Escape::Leaves(("returned and stored".to_owned(), None)),
            (true, None) => Escape::Returned,
            (false, Some(into)) => into,
            (false, None) => Escape::Stays,
        }
    }

    /// Whether the use `u` of a local is held by the value assigned to that same local.
    fn held_by_itself(&self, u: ExprId) -> bool {
        let hir::ExprKind::Local(x) = self.body.expr(u).kind else { return false };
        let top = self.holder(u);
        let Some(Up::Stmt(s)) = self.up.get(&top) else { return false };
        matches!(self.body.stmt(*s).kind,
            hir::StmtKind::Assign { target, value, op: hir::AssignOp::Assign, .. }
            if value == top && matches!(self.body.expr(target).kind, hir::ExprKind::Local(l) if l == x))
    }

    /// The local `target` is, when it is a whole local this body declares by value with a
    /// `let`: what an assignment's value may be followed into, as a `let`'s is.
    fn assigned_local(&self, target: ExprId) -> Option<LocalId> {
        let hir::ExprKind::Local(y) = self.body.expr(target).kind else { return None };
        let by_value =
            !matches!(self.body.local(y).ty, Ty::Ref { .. } | Ty::Ptr { .. } | Ty::OwnPtr { .. });
        (by_value && self.declared_in(y).is_some()).then_some(y)
    }

    /// Where a value stored into `container` is proposed to go (T-0145): where the
    /// container lives, which is its `let`'s block, or the body's result placement when the
    /// container is what a result function returns. `site` is the block the value is made
    /// in. A value made on every pass of a loop that block is outside of goes there only
    /// when it is stored directly, by a method that keeps it, into a container that keeps
    /// everything ([`crate::storing::keeps`]); otherwise the region would grow with every
    /// pass, past what the container holds.
    fn stored_target(
        &self,
        container: LocalId,
        site: BlockId,
        direct: bool,
        kept: bool,
    ) -> Proposal {
        let name = self.body.local(container).name.clone();
        let Some(block) = self.declared_in(container) else {
            return Err((format!("stored in `{name}`, which is not a `let` of this body"), None));
        };
        let target = match self.escape_of(container, 0) {
            Escape::Stays => Target::Block(block),
            Escape::Returned => Target::Result,
            Escape::Into { .. } | Escape::Held(_) => {
                return Err((format!("stored in `{name}`, which is stored in turn"), None))
            }
            Escape::Leaves((why, at)) => return Err((format!("stored in `{name}`: {why}"), at)),
        };
        if let Some((why, at)) = self.lets_go(container) {
            return Err((format!("{why} `{name}`"), at));
        }
        let outer = match target {
            Target::Block(b) => b,
            Target::Result => self.body.root,
        };
        match self.loop_between(site, outer) {
            None => Err((format!("made before `{name}`'s block"), None)),
            Some(false) => Ok(target),
            Some(true)
                if direct
                    && kept
                    && crate::storing::keeps(self.body, self.table, self.res, container) =>
            {
                Ok(target)
            }
            Some(true) => Err((format!("stored in `{name}` on every pass of a loop"), None)),
        }
    }

    /// When argument `e` of a call is stored into the call's receiver, a container this body
    /// declares (T-0145): which, and how ([`Escape::Into`]). `depth` is how many `let`s
    /// the value has passed through on its way here. A method that may drop what it is
    /// given keeps it at `call` when it adds a key just looked up and not found
    /// ([`crate::storing::adds_new`]).
    fn stored_by(
        &self,
        call: ExprId,
        callee: &Callee,
        args: &[ExprId],
        e: ExprId,
        depth: u32,
    ) -> Option<Escape> {
        let i = args.iter().position(|a| *a == e)?;
        let kept = self.staying.stores(self.res, self.module, callee, i)?
            || crate::storing::adds_new(self.body, self.res, call);
        let Callee::Method { recv, .. } = callee else { return None };
        let container = self.receiver_local(*recv)?;
        Some(Escape::Into { container, direct: depth == 0, kept })
    }

    /// Whether what is stored into `container` can leave it for somewhere a region cannot
    /// follow (T-0145): the container lent as `&mut`, or an element taken back out (a
    /// `&mut self` method returning something that owns memory: `pop`, `remove`) and then
    /// not kept here. Why, as "…" before the container's name, and where.
    fn lets_go(&self, container: LocalId) -> Option<(String, Option<Span>)> {
        let uses = (0..self.body.exprs.len() as u32).map(ExprId).filter(
            |e| matches!(self.body.expr(*e).kind, hir::ExprKind::Local(l) if l == container),
        );
        for u in uses {
            let mut top = u;
            while let Some(Up::Expr(p)) = self.up.get(&top) {
                let place = matches!(self.body.expr(*p).kind,
                    hir::ExprKind::Field { base, .. } | hir::ExprKind::TupleIndex { base, .. }
                    if base == top);
                if !place {
                    break;
                }
                top = *p;
            }
            let Some(Up::Expr(p)) = self.up.get(&top) else { continue };
            let span = Some(self.body.expr(*p).span);
            match &self.body.expr(*p).kind {
                hir::ExprKind::Ref { mutable: true, .. } => {
                    return Some(("lent as `&mut` with".to_owned(), span))
                }
                hir::ExprKind::Call {
                    callee: Callee::Method { recv, owner, name, .. }, ..
                } if *recv == top => {
                    let mutates = self
                        .table
                        .methods_sig(*owner)
                        .and_then(|s| s.methods.get(name))
                        .is_some_and(|sig| sig.takes_self == Some(true));
                    if !mutates || !crate::storing::owns(self.table, self.body.ty(*p)) {
                        continue;
                    }
                    match self.fate(*p, 1) {
                        Escape::Stays => {}
                        Escape::Leaves((why, at)) => {
                            return Some((format!("{why}, once taken out of"), at))
                        }
                        Escape::Returned | Escape::Into { .. } | Escape::Held(_) => {
                            return Some(("moved on, once taken out of".to_owned(), span))
                        }
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// The block whose `let` declares `local`.
    fn declared_in(&self, local: LocalId) -> Option<BlockId> {
        (0..self.body.stmts.len() as u32).map(StmtId).find_map(|s| match self.body.stmt(s).kind {
            hir::StmtKind::Let { local: Some(l), .. } if l == local => {
                self.stmt_block.get(&s).copied()
            }
            _ => None,
        })
    }

    /// Whether a loop lies between block `inner` and block `outer` around it, `inner`
    /// counted and `outer` not; `None` when `outer` is not around `inner`.
    fn loop_between(&self, mut inner: BlockId, outer: BlockId) -> Option<bool> {
        let mut looped = false;
        loop {
            if inner == outer {
                return Some(looped);
            }
            let (kind, parent, _) = self.blocks.get(&inner)?;
            looped |= *kind == Kind::Loop;
            inner = (*parent)?;
        }
    }

    /// The local a method's receiver is a by-value field path of, when the body declares
    /// it with a `let`: `names` in `names.push(…)` and `store.names.push(…)`.
    fn receiver_local(&self, recv: ExprId) -> Option<LocalId> {
        match self.body.expr(recv).kind {
            hir::ExprKind::Local(l) => {
                let local = self.body.local(l);
                let by_value =
                    !matches!(local.ty, Ty::Ref { .. } | Ty::Ptr { .. } | Ty::OwnPtr { .. });
                (by_value && self.declared_in(l).is_some()).then_some(l)
            }
            hir::ExprKind::Field { base, .. } | hir::ExprKind::TupleIndex { base, .. } => {
                if matches!(self.body.ty(base), Ty::Ref { .. } | Ty::Ptr { .. } | Ty::OwnPtr { .. })
                {
                    return None;
                }
                self.receiver_local(base)
            }
            _ => None,
        }
    }

    /// The outermost value holding `e`'s, through what moves it whole into another: a
    /// `move`, an `or` (its operand or its fallback), a struct or union literal, a tuple, an
    /// array, and the value a `Box` is made of.
    fn holder(&self, mut e: ExprId) -> ExprId {
        while let Some(Up::Expr(p)) = self.up.get(&e) {
            if self.holds(*p, e) {
                e = *p;
            } else {
                break;
            }
        }
        e
    }

    /// Whether `parent` holds `e`'s value as its own: what [`Self::holder`] climbs.
    fn holds(&self, parent: ExprId, e: ExprId) -> bool {
        holds_part(self.body, self.res, parent, e)
    }

    /// Whether `e` reads a place by value without moving it: for a copyable type, a copy.
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

    /// What becomes of `e`'s value, followed through whatever holds it ([`Self::holds`])
    /// and into a local a `let` moves it to: it stays where it is (borrowed, copied,
    /// assigned to, dropped with its block), it is returned, or it leaves, and why and
    /// where. A return is [`Escape::Returned`] only in a result function; elsewhere the
    /// value leaves with it. `depth` bounds following one local into the next.
    fn fate(&self, e: ExprId, depth: u32) -> Escape {
        self.fate_as(e, depth, self.is_copy(e))
    }

    /// [`Self::fate`], with `copy` saying whether `e` is read as a copy, which leaves the
    /// place it reads where it is. A copy site's own value is not one: it is followed as a
    /// value of its own (T-0140).
    fn fate_as(&self, e: ExprId, depth: u32, copy: bool) -> Escape {
        let span = |x: ExprId| Some(self.body.expr(x).span);
        let leaves = |why: String, at: Option<Span>| {
            if copy {
                Escape::Stays
            } else {
                Escape::Leaves((why, at))
            }
        };
        let Some(up) = self.up.get(&e) else {
            return Escape::Leaves(("used where this cannot follow it".to_owned(), span(e)));
        };
        match *up {
            Up::Stmt(s) => {
                let stmt = self.body.stmt(s);
                match &stmt.kind {
                    hir::StmtKind::Let { local: Some(y), init: Some(init) }
                        if *init == e && !copy && depth < 8 =>
                    {
                        // Moved into another local, which holds it from here on.
                        self.escape_of(*y, depth + 1)
                    }
                    hir::StmtKind::Let { local, .. } => {
                        let name =
                            local.map_or("_".to_owned(), |l| self.body.local(l).name.clone());
                        leaves(format!("moved to `{name}`"), Some(stmt.span))
                    }
                    // Each part goes to its own local, which holds it from here on (T-0146).
                    hir::StmtKind::LetTuple { locals, init }
                        if *init == e && !copy && depth < 8 =>
                    {
                        let locals: Vec<LocalId> = locals.iter().flatten().copied().collect();
                        self.escape_of_all(&locals, depth + 1)
                    }
                    hir::StmtKind::LetTuple { .. } => {
                        leaves("moved into a tuple".to_owned(), Some(stmt.span))
                    }
                    hir::StmtKind::Assign { target, .. } if *target == e => Escape::Stays,
                    // Assigned to a local of the body, which holds it from here on as a
                    // `let`'s would (T-0146).
                    hir::StmtKind::Assign { target, value, op: hir::AssignOp::Assign, .. }
                        if *value == e && !copy && depth < 8 =>
                    {
                        match self.assigned_local(*target) {
                            Some(y) => match self.escape_of(y, depth + 1) {
                                Escape::Stays => Escape::Held(y),
                                other => other,
                            },
                            None => {
                                let name =
                                    place_name(self.res, self.body, Some(self.table), *target);
                                leaves(format!("stored in `{name}`"), Some(stmt.span))
                            }
                        }
                    }
                    hir::StmtKind::Assign { target, .. } => {
                        let name = place_name(self.res, self.body, Some(self.table), *target);
                        leaves(format!("stored in `{name}`"), Some(stmt.span))
                    }
                    hir::StmtKind::Expr(_) if self.is_tail_value(s) => {
                        Escape::Leaves(("the value of a block".to_owned(), span(e)))
                    }
                    hir::StmtKind::For { shape: hir::ForShape::Consuming, .. } => {
                        Escape::Leaves(("consumed by a `for`".to_owned(), Some(stmt.span)))
                    }
                    _ => Escape::Stays,
                }
            }
            Up::Expr(p) => {
                // `move(x)` takes `x` whole, copyable or not; anything else that holds a
                // copy holds a copy, which leaves `x` where it is.
                let moved = matches!(self.body.expr(p).kind, hir::ExprKind::Move(_));
                if (moved || !copy) && self.holds(p, e) {
                    return self.fate(p, depth);
                }
                let parent = self.body.expr(p);
                match &parent.kind {
                    hir::ExprKind::Ref { .. }
                    | hir::ExprKind::Len(_)
                    | hir::ExprKind::DataPtr(_)
                    | hir::ExprKind::Deref(_)
                    | hir::ExprKind::Switch { .. }
                    | hir::ExprKind::If { .. } => Escape::Stays,
                    hir::ExprKind::Index { base, .. } | hir::ExprKind::SliceRange { base, .. }
                        if *base == e =>
                    {
                        Escape::Stays
                    }
                    // A field read follows the field: a copy of it leaves the value alone.
                    hir::ExprKind::Field { .. } | hir::ExprKind::TupleIndex { .. } => {
                        self.fate(p, depth)
                    }
                    hir::ExprKind::Call { callee, args } => match callee {
                        // Every Dazzle method borrows its receiver (spec §7.3).
                        Callee::Method { recv, .. } | Callee::BaseMethod { recv, .. }
                            if *recv == e =>
                        {
                            Escape::Stays
                        }
                        _ if args.iter().position(|a| *a == e).is_some_and(|i| {
                            self.staying.stays(self.res, self.module, callee, i)
                        }) =>
                        {
                            Escape::Stays
                        }
                        _ => match self.stored_by(p, callee, args, e, depth) {
                            Some(into) if !copy => into,
                            _ => leaves(format!("passed to `{}`", self.call_name(p)), span(p)),
                        },
                    },
                    hir::ExprKind::Return(_) => {
                        // C++ moves a local it returns, so that is never a copy.
                        let moved = matches!(self.body.expr(e).kind, hir::ExprKind::Local(_));
                        if copy && !moved {
                            Escape::Stays
                        } else if self.returns_result {
                            Escape::Returned
                        } else {
                            Escape::Leaves(("returned".to_owned(), span(p)))
                        }
                    }
                    hir::ExprKind::IfLet { binding, .. } => {
                        if matches!(self.body.local(*binding).ty, Ty::Ref { .. }) {
                            Escape::Stays
                        } else {
                            leaves("taken out by `if let`".to_owned(), span(p))
                        }
                    }
                    hir::ExprKind::Or { .. } => leaves("taken out by `or`".to_owned(), span(p)),
                    hir::ExprKind::Formatted { .. } => {
                        leaves("passed to a formatting macro".to_owned(), span(p))
                    }
                    hir::ExprKind::Builtin { name, args }
                    | hir::ExprKind::Intrinsic { name, args } => {
                        let in_place = args.first() == Some(&e)
                            && matches!(name.as_str(), "construct" | "destroy" | "move_out");
                        if in_place {
                            Escape::Stays
                        } else {
                            leaves(format!("passed to `{name}`"), span(p))
                        }
                    }
                    _ => leaves("used by an operator".to_owned(), span(p)),
                }
            }
        }
    }

    /// Whether `s` is the last statement of a block whose value is used.
    fn is_tail_value(&self, s: StmtId) -> bool {
        let Some(b) = self.stmt_block.get(&s) else { return false };
        if self.body.block(*b).stmts.last() != Some(&s) {
            return false;
        }
        match self.blocks.get(b) {
            Some((_, _, Some(by))) => {
                !matches!(self.body.ty(*by), Ty::Void | Ty::Never | Ty::Error)
            }
            _ => false,
        }
    }

    // --- words --------------------------------------------------------------------------

    fn call_name(&self, call: ExprId) -> String {
        match &self.body.expr(call).kind {
            hir::ExprKind::Call { callee, .. } => match callee {
                Callee::Method { recv, name, .. } | Callee::BaseMethod { recv, name, .. } => {
                    format!("{}.{name}", place_name(self.res, self.body, Some(self.table), *recv))
                }
                _ => callee_name(self.res, self.body, callee, self.body.ty(call)),
            },
            hir::ExprKind::Builtin { name, .. } | hir::ExprKind::Intrinsic { name, .. } => {
                name.clone()
            }
            _ => "…".to_owned(),
        }
    }

    /// The function the body is, as `region: ` names it.
    fn function(&self) -> String {
        let sym = |s: &SymbolId| self.res.symbol(*s).name.clone();
        match &self.body.owner {
            BodyOwner::Fn(s) => format!("`{}`", sym(s)),
            BodyOwner::Method { name, .. }
            | BodyOwner::TraitDefaults { name, .. }
            | BodyOwner::ImplMethod { name, .. } => format!("`{name}`"),
            BodyOwner::Ctor { owner, .. } => format!("the `{}` constructor", sym(owner)),
            BodyOwner::Copy(owner) => format!("`{}`'s copy", sym(owner)),
            BodyOwner::Drop(owner) => format!("`{}`'s drop", sym(owner)),
            BodyOwner::Const(s) | BodyOwner::Static(s) => format!("`{}`", sym(s)),
            BodyOwner::AssocConst { name, .. } => format!("`{name}`"),
            BodyOwner::FieldDefault { owner, .. } => format!("`{}`", sym(owner)),
        }
    }

    /// Which region a block's is, for a reader.
    fn scope(&self, block: BlockId) -> String {
        let f = self.function();
        match self.blocks.get(&block).map(|(k, _, _)| *k) {
            Some(Kind::Root) | None => f,
            Some(Kind::Loop) => format!("each pass of a loop in {f}"),
            Some(Kind::If) => format!("an `if` in {f}"),
            Some(Kind::Case) => format!("a `case` in {f}"),
            Some(Kind::Or) => format!("an `or` block in {f}"),
            Some(Kind::Plain) | Some(Kind::Lambda) => format!("a block in {f}"),
        }
    }

    fn refusal(&self, why: &Refusal) -> String {
        match why {
            Refusal::Returned => "returned".to_owned(),
            Refusal::Stored(target) => {
                format!(
                    "stored in `{}`",
                    place_name(self.res, self.body, Some(self.table), *target)
                )
            }
            Refusal::Passed(call) => match self.body.expr(*call).kind {
                hir::ExprKind::Formatted { .. } => "passed to a formatting macro".to_owned(),
                _ => format!("passed to `{}`", self.call_name(*call)),
            },
            Refusal::Outlives(block) => format!("outlives {}", self.scope(*block)),
            Refusal::EveryPass => "made on every pass of a loop".to_owned(),
            Refusal::Unfollowed(what) => (*what).to_owned(),
        }
    }
}
