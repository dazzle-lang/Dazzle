//! Drops a region makes unnecessary (spec §14.6, T-0118, r90).
//!
//! When a block's region is released, everything in it goes at once, so the drop of a
//! value whose memory is all there need not run, provided all it would do is free that
//! memory. Two questions decide it, and this module answers both:
//!
//! 1. **Does the type's drop only free?** Proven per type instance, from the drop bodies
//!    themselves, never from a marker a type writes ([`Drops::only_frees`]). A drop only
//!    frees when its body does nothing but give memory back through `Allocator::free`
//!    and destroy parts whose own drops only free. `List<String>`, `HashMap<i32, i32>` and
//!    `String` do; `File` (it closes a handle), `Rc` (a count other owners see), and any
//!    drop that calls C, another function, a function value or a trait object do not. The
//!    one function value followed is `Box`'s, which is the drop of what the box was made
//!    as.
//! 2. **Is everything the value owns in the region?** Placement says where the value was
//!    built; this asks whether anything it owns can have come from elsewhere, or been put
//!    there since ([`Drops::releasable`], [`skipped_drops`]). That is a value whose parts
//!    own nothing of their own (`String`, `List<i32>`, a struct of those), built whole in
//!    the region and never replaced; or a container of values that do (`List<String>`)
//!    when everything put into it was built whole in the same region, which placement does
//!    for a value stored into a container (T-0145), and nothing could have put anything
//!    else there ([`Contents`]).
//!
//! A drop skipped wrongly would leak, not corrupt memory. Either way, when unsure, both
//! questions answer "no", which only keeps a drop.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use dz_check::{ItemSig, TypeTable};
use dz_hir::ty::Ty;
use dz_hir::{self as hir, BlockId, Body, BodyOwner, Callee, ExprId, Hir, LocalId, StmtId};
use dz_resolve::{Resolution, SymbolId};

use crate::placement::{callee_owner, self_field, Decision, Placements};

/// What is known about drops, worked out once per type and kept.
pub struct Drops<'a> {
    hir: &'a Hir,
    table: &'a TypeTable,
    res: &'a Resolution,
    allocator: Option<SymbolId>,
    boxed: Option<SymbolId>,
    only_frees: Memo,
    releasable: Memo,
    sealed: RefCell<HashMap<SymbolId, bool>>,
}

/// Answers by type. `Ty` has no hash, and a program asks about few types.
#[derive(Default)]
struct Memo(RefCell<Vec<(Ty, bool)>>);

impl Memo {
    fn get(&self, ty: &Ty) -> Option<bool> {
        self.0.borrow().iter().find(|(t, _)| t == ty).map(|(_, b)| *b)
    }

    fn set(&self, ty: &Ty, answer: bool) {
        let mut known = self.0.borrow_mut();
        match known.iter_mut().find(|(t, _)| t == ty) {
            Some(entry) => entry.1 = answer,
            None => known.push((ty.clone(), answer)),
        }
    }

    /// The memoised answer for `ty`, or `compute`'s. While it is being computed, `ty` is
    /// taken to answer `false`: a type reached again through itself, which only ever keeps
    /// a drop.
    fn answer(&self, ty: &Ty, compute: impl FnOnce() -> bool) -> bool {
        if let Some(known) = self.get(ty) {
            return known;
        }
        self.set(ty, false);
        let answer = compute();
        self.set(ty, answer);
        answer
    }
}

impl<'a> Drops<'a> {
    pub fn new(hir: &'a Hir, table: &'a TypeTable, res: &'a Resolution) -> Self {
        let boxed = res.lookup(res.prelude, "Box");
        Drops {
            hir,
            table,
            res,
            allocator: res.lookup(res.prelude, "Allocator"),
            boxed,
            only_frees: Memo::default(),
            releasable: Memo::default(),
            sealed: RefCell::default(),
        }
    }

    /// Whether a value of `ty` owns something its drop gives back: what `needs_drop` says,
    /// and an optional or fallible value of such a type too.
    fn owns(&self, ty: &Ty) -> bool {
        match ty {
            Ty::Optional(inner) | Ty::Fallible(inner) => self.owns(inner),
            _ => self.table.needs_drop(ty),
        }
    }

    /// Whether dropping a value of `ty` does nothing but free memory it owns (r90): its own
    /// drop, its bases' and its parts' all only free. A type with no drop at all does. A
    /// type parameter does not, since it could be anything; nor does a C++ class.
    pub fn only_frees(&self, ty: &Ty) -> bool {
        if !self.owns(ty) {
            return true;
        }
        self.only_frees.answer(ty, || self.only_frees_now(ty))
    }

    fn only_frees_now(&self, ty: &Ty) -> bool {
        match ty {
            Ty::Optional(inner) | Ty::Fallible(inner) => self.only_frees(inner),
            Ty::Array { elem, .. } => self.only_frees(elem),
            Ty::Tuple(parts) => parts.iter().all(|t| self.only_frees(t)),
            Ty::Named { sym, args } if !ty.has_params() => match self.table.get(*sym) {
                Some(ItemSig::Struct(s)) => {
                    if s.opaque || s.cpp_owned {
                        return false;
                    }
                    // A base's drop runs as part of this one, after it (spec §7.3).
                    for b in self.table.with_bases(*sym) {
                        if self.table.struct_sig(b).is_some_and(|s| s.drop.is_some()) {
                            let b_args = if b == *sym {
                                args.clone()
                            } else {
                                self.table.args_as(*sym, args, b)
                            };
                            if !self.drop_only_frees(b, &b_args) {
                                return false;
                            }
                        }
                    }
                    self.table.field_types(*sym, args, &s.fields).iter().all(|t| self.only_frees(t))
                }
                Some(ItemSig::Union { variants, .. }) => variants.iter().all(|v| {
                    self.table.payload_types(*sym, args, v).iter().all(|t| self.only_frees(t))
                }),
                _ => false,
            },
            _ => false,
        }
    }

    /// Whether `sym`'s own `drop`, as the instance `args` sees it, only frees.
    fn drop_only_frees(&self, sym: SymbolId, args: &[Ty]) -> bool {
        let Some(body) = self.hir.find(&BodyOwner::Drop(sym)) else { return false };
        let self_ty = Ty::Named { sym, args: args.to_vec() };
        let map = self
            .table
            .drop_impl(sym)
            .and_then(|i| self.table.impl_applies(&self.table.impls()[i], &self_ty))
            .unwrap_or_default();
        let body = hir::map_body_types(body, |t| self.table.subst_with(t, &map, Some(&self_ty)));
        // `Box`'s drop calls the drop of what it was made as, kept in `m_drop`.
        let boxed = (Some(sym) == self.boxed).then(|| args.first().cloned()).flatten();
        Frees { d: self, body: &body, boxed }.block(body.root)
    }

    /// Whether the drop a `Box<T>` keeps only frees: `T`'s, or that of any struct deriving
    /// from `T`, since a `Box<Button>` moves into a `Box<UiElement>` (spec §14.5).
    fn boxed_only_frees(&self, t: &Ty) -> bool {
        if !self.only_frees(t) {
            return false;
        }
        let Ty::Named { sym: base, .. } = t else { return true };
        !self.res.symbols.iter().enumerate().any(|(i, s)| {
            let sym = SymbolId(i as u32);
            s.kind == dz_resolve::SymbolKind::Struct
                && sym != *base
                && self.table.with_bases(sym).contains(base)
        })
    }

    /// A container: a struct with a drop of its own that only frees, no base, and nothing in
    /// its fields or type arguments that owns anything. What it owns is memory it took
    /// from where it was placed, the contract §14.6 gives a container reading
    /// `#placement`: `List<i32>`, `HashMap<i32, i32>`, `List<u8>`.
    fn leaf(&self, ty: &Ty) -> bool {
        let Ty::Named { sym, args } = ty else { return false };
        let Some(s) = self.table.struct_sig(*sym) else { return false };
        s.drop.is_some()
            && s.base.is_none()
            && args.iter().all(|t| !self.owns(t))
            && self.table.field_types(*sym, args, &s.fields).iter().all(|t| !self.owns(t))
            && self.only_frees(ty)
    }

    /// Whether everything a value of `ty` owns lives where the value was built, for as long
    /// as nothing outside the type replaces its parts: a container ([`Self::leaf`]), or a
    /// struct with no drop of its own whose owning fields are such values, private, and
    /// never replaced by its own methods ([`Self::sealed`]). A type that owns nothing has
    /// no drop to skip, so it is not.
    pub fn releasable(&self, ty: &Ty) -> bool {
        if ty.has_params() || !self.owns(ty) || !self.only_frees(ty) {
            return false;
        }
        self.releasable.answer(ty, || self.leaf(ty) || self.composite(ty))
    }

    /// A container of values that own memory: a struct with a drop of its own that only
    /// frees, no base, fields that own nothing, and type arguments, at least one owning,
    /// each [`Self::releasable`] (`List<String>`, `HashMap<i32, String>`). Its own memory is
    /// where it was placed, as for [`Self::leaf`]; whether its elements are there too
    /// depends on what was put into it, which [`skipped_drops`] asks of each local (T-0145).
    fn holder(&self, ty: &Ty) -> bool {
        let Ty::Named { sym, args } = ty else { return false };
        let Some(s) = self.table.struct_sig(*sym) else { return false };
        !ty.has_params()
            && s.drop.is_some()
            && s.base.is_none()
            && args.iter().any(|t| self.owns(t))
            && args.iter().all(|t| !self.owns(t) || self.releasable(t))
            && self.table.field_types(*sym, args, &s.fields).iter().all(|t| !self.owns(t))
            && self.only_frees(ty)
    }

    fn composite(&self, ty: &Ty) -> bool {
        let Ty::Named { sym, args } = ty else { return false };
        let Some(s) = self.table.struct_sig(*sym) else { return false };
        if s.drop.is_some() || s.base.is_some() || s.opaque || s.cpp_owned || s.is_union {
            return false;
        }
        // A `pub` field may be written from outside the type too, but only through a value
        // its caller holds: [`skipped_drops`] refuses a local any of whose owning parts is
        // assigned or handed on as `&mut`.
        let types = self.table.field_types(*sym, args, &s.fields);
        types.iter().all(|t| !self.owns(t) || self.releasable(t)) && self.sealed(*sym)
    }

    /// Whether no method of `sym`'s own, of an implementation for it, or a default one it
    /// inherits from a trait, replaces an owning field of `self` or `self` itself: an
    /// assignment to one, a `&mut` taken of one to hand on, or `construct` into one. Its
    /// constructors build those fields, and its copy runs only on a value being made.
    fn sealed(&self, sym: SymbolId) -> bool {
        if let Some(&known) = self.sealed.borrow().get(&sym) {
            return known;
        }
        let impls: Vec<usize> = (0..self.table.impls().len())
            .filter(|i| {
                matches!(&self.table.impls()[*i].self_ty, Ty::Named { sym: s, .. } if *s == sym)
            })
            .collect();
        let traits: HashSet<SymbolId> =
            impls.iter().map(|i| self.table.impls()[*i].trait_ref.trait_sym).collect();
        let ours = |owner: &BodyOwner| match owner {
            BodyOwner::Method { owner, .. } => *owner == sym,
            BodyOwner::ImplMethod { imp, .. } => impls.contains(&(*imp as usize)),
            BodyOwner::TraitDefaults { owner, .. } => traits.contains(owner),
            _ => false,
        };
        let answer =
            self.hir.bodies().filter(|(_, b)| ours(&b.owner)).all(|(_, b)| !replaces_self(self, b));
        self.sealed.borrow_mut().insert(sym, answer);
        answer
    }
}

/// Whether a method body may replace an owning part of `self`, or `self` itself.
fn replaces_self(d: &Drops<'_>, body: &Body) -> bool {
    let is_self = |e: ExprId| match body.expr(e).kind {
        hir::ExprKind::Local(l) => body.local(l).name == "self",
        hir::ExprKind::Deref(inner) => {
            matches!(body.expr(inner).kind, hir::ExprKind::Local(l) if body.local(l).name == "self")
        }
        _ => false,
    };
    // `self` as a whole counts whatever its type says: in a trait's default it is `Self`.
    let owning = |e: ExprId| is_self(e) || (self_field(body, e) && d.owns(body.ty(e)));
    let stmts = (0..body.stmts.len() as u32).map(StmtId);
    let assigned = stmts.into_iter().any(
        |s| matches!(body.stmt(s).kind, hir::StmtKind::Assign { target, .. } if owning(target)),
    );
    let handed = (0..body.exprs.len() as u32).map(ExprId).any(|e| match &body.expr(e).kind {
        hir::ExprKind::Ref { mutable: true, place, .. } => owning(*place),
        hir::ExprKind::Builtin { name, args } | hir::ExprKind::Intrinsic { name, args } => {
            matches!(name.as_str(), "construct" | "destroy" | "move_out")
                && args.first().is_some_and(|a| owning(*a))
        }
        _ => false,
    });
    assigned || handed
}

/// The walk over one drop body, instance-substituted, that proves it only frees.
struct Frees<'d, 'a> {
    d: &'d Drops<'a>,
    body: &'d Body,
    /// In `Box<T>`'s drop, `T`: the function value it calls is `T`'s drop, or a derived
    /// type's.
    boxed: Option<Ty>,
}

impl Frees<'_, '_> {
    fn block(&self, b: BlockId) -> bool {
        self.body.block(b).stmts.iter().all(|s| self.stmt(*s))
    }

    fn stmt(&self, s: StmtId) -> bool {
        match &self.body.stmt(s).kind {
            hir::StmtKind::Let { init, .. } => init.map_or(true, |e| self.expr(e)),
            hir::StmtKind::LetTuple { init, .. } => self.expr(*init),
            // Writing a local, or a field of the value being dropped, is gone with it.
            // Writing through a pointer is not: it may be memory someone else sees.
            hir::StmtKind::Assign { target, value, .. } => {
                let local = matches!(self.body.expr(*target).kind,
                    hir::ExprKind::Local(l) if self.body.local(l).name != "self");
                (local || self_field(self.body, *target)) && self.expr(*value)
            }
            hir::StmtKind::Expr(e) => self.expr(*e),
            hir::StmtKind::While { cond, body, .. } => self.expr(*cond) && self.block(*body),
            // A count, `for i in 0..n`, and nothing that calls an iterator.
            hir::StmtKind::For { subject, body, shape, .. } => {
                *shape == hir::ForShape::Single
                    && matches!(self.body.expr(*subject).kind, hir::ExprKind::Range { .. })
                    && self.expr(*subject)
                    && self.block(*body)
            }
            hir::StmtKind::Defer(inner) => self.stmt(*inner),
            hir::StmtKind::Item(_) => true,
            hir::StmtKind::Error => false,
        }
    }

    fn expr(&self, e: ExprId) -> bool {
        use hir::ExprKind as K;
        let all = |xs: &[ExprId]| xs.iter().all(|x| self.expr(*x));
        match &self.body.expr(e).kind {
            K::Int(_)
            | K::Float(_)
            | K::Bool(_)
            | K::Char(_)
            | K::Str(_)
            | K::Null
            | K::NoneOpt
            | K::Local(_)
            | K::Const(_)
            | K::Static(_)
            | K::AssocConst { .. }
            | K::FnItem { .. }
            | K::EnumVariant { .. }
            | K::TypeQuery { .. }
            | K::DefaultArg(_)
            | K::Break(_)
            | K::Continue(_) => true,
            K::Field { base, .. }
            | K::TupleIndex { base, .. }
            | K::Deref(base)
            | K::Len(base)
            | K::DataPtr(base)
            | K::Ref { place: base, .. }
            | K::Unary { operand: base, .. }
            | K::Cast { operand: base, .. } => self.expr(*base),
            K::Index { base, index } => self.expr(*base) && self.expr(*index),
            K::SliceRange { base, start, end } => {
                self.expr(*base) && [start, end].into_iter().flatten().all(|x| self.expr(*x))
            }
            K::Binary { lhs, rhs, .. } => self.expr(*lhs) && self.expr(*rhs),
            K::Range { start, end, .. } => {
                [start, end].into_iter().flatten().all(|x| self.expr(*x))
            }
            // A value made here is dropped here, so its own drop must only free; moving a
            // part out drops it the same way.
            K::StructLit { fields, .. } | K::UnionVariant { payload: fields, .. } => {
                self.d.only_frees(self.body.ty(e)) && fields.iter().flatten().all(|x| self.expr(*x))
            }
            K::Tuple(parts) | K::ArrayLit(parts) => {
                self.d.only_frees(self.body.ty(e)) && all(parts)
            }
            K::Move(inner) => self.d.only_frees(self.body.ty(e)) && self.expr(*inner),
            K::Block(b) => self.block(*b),
            K::If { cond, then, else_branch } => match self.fold(*cond) {
                Some(true) => self.block(*then),
                Some(false) => else_branch.map_or(true, |b| self.block(b)),
                None => {
                    self.expr(*cond)
                        && self.block(*then)
                        && else_branch.map_or(true, |b| self.block(b))
                }
            },
            K::IfLet { binding, operand, then, else_branch } => {
                self.d.only_frees(&self.body.local(*binding).ty)
                    && self.expr(*operand)
                    && self.block(*then)
                    && else_branch.map_or(true, |b| self.block(b))
            }
            K::Or { operand, clause } => {
                self.expr(*operand)
                    && match clause {
                        hir::OrClause::Value(v) => self.expr(*v),
                        hir::OrClause::Block { body, .. } => self.block(*body),
                        hir::OrClause::Ignore | hir::OrClause::Unreachable => true,
                    }
            }
            K::Return(v) => v.map_or(true, |v| self.expr(v)),
            K::Call { callee, args } => all(args) && self.call(callee),
            K::Builtin { name, args } | K::Intrinsic { name, args } => match name.as_str() {
                "destroy" => {
                    args.len() == 1 && self.d.only_frees(self.body.ty(args[0])) && all(args)
                }
                "sizeof" | "alignof" => true,
                _ => false,
            },
            // A handle's guard is a strong count; formatting writes somewhere; a `switch`
            // is not met in a drop that only frees, so it is not followed.
            K::Guarded { .. }
            | K::Formatted { .. }
            | K::Switch { .. }
            | K::Reflect { .. }
            | K::Error => false,
        }
    }

    /// The one call a drop that only frees makes: `Allocator::free`, through the allocator
    /// it was placed in, and in `Box`'s drop the drop of what it holds.
    fn call(&self, callee: &Callee) -> bool {
        match callee {
            Callee::Method { recv, owner, name, .. } => {
                Some(*owner) == self.d.allocator && name == "free" && self.expr(*recv)
            }
            Callee::Value(v) => {
                self.boxed.as_ref().is_some_and(|t| self.d.boxed_only_frees(t))
                    && self.reads_field(*v, "m_drop")
            }
            _ => false,
        }
    }

    /// Whether `e` is the field `name` of the value being dropped, read directly or through
    /// a `let` that holds it.
    fn reads_field(&self, e: ExprId, name: &str) -> bool {
        let field = |x: ExprId| {
            self_field(self.body, x)
                && matches!(self.body.expr(x).kind, hir::ExprKind::Field { base, index }
                    if field_name(self.d.table, self.body.ty(base), index).as_deref() == Some(name))
        };
        match self.body.expr(e).kind {
            hir::ExprKind::Local(l) => {
                (0..self.body.stmts.len() as u32).map(StmtId).any(|s| {
                    matches!(self.body.stmt(s).kind, hir::StmtKind::Let { local: Some(x), init: Some(i) }
                        if x == l && field(i))
                })
            }
            _ => field(e),
        }
    }

    /// An `if` condition made only of type questions, answered for this instance, so the
    /// branch it never takes is not followed (spec §8.3): `!T::is_trivially_copyable`.
    fn fold(&self, e: ExprId) -> Option<bool> {
        use hir::ExprKind as K;
        match &self.body.expr(e).kind {
            K::Bool(b) => Some(*b),
            K::TypeQuery { ty, query } if !ty.has_params() => match query {
                hir::TypeQuery::IsTriviallyCopyable => Some(self.d.table.trivially_copyable(ty)),
                hir::TypeQuery::IsCopyable => Some(self.d.table.copyable(ty)),
                hir::TypeQuery::HasDrop => Some(self.d.table.needs_drop(ty)),
                _ => None,
            },
            K::Unary { op: dz_ast::UnaryOp::Not, operand } => self.fold(*operand).map(|b| !b),
            K::Binary { op: dz_ast::BinaryOp::And, lhs, rhs } => {
                Some(self.fold(*lhs)? && self.fold(*rhs)?)
            }
            K::Binary { op: dz_ast::BinaryOp::Or, lhs, rhs } => {
                Some(self.fold(*lhs)? || self.fold(*rhs)?)
            }
            _ => None,
        }
    }
}

fn field_name(table: &TypeTable, base: &Ty, index: usize) -> Option<String> {
    match base.peeled_refs() {
        Ty::Named { sym, .. } => table.struct_sig(*sym)?.fields.get(index).map(|f| f.name.clone()),
        _ => None,
    }
}

/// Which locals of each body are dropped by their region's release rather than by their
/// drop, recorded on the body's placement with the sites they hold (spec §14.6, T-0118).
///
/// A local `x` qualifies when:
///
/// - `let x = …` builds it whole in its block's region: its value is a site placed there
///   whose callee builds everything it owns in its placement ([`whole_functions`]), or a
///   struct literal whose owning fields are such sites;
/// - its type is concrete and [`Drops::releasable`];
/// - nothing in the body replaces it or an owning part of it: no assignment, no `&mut`
///   handed on (the receiver of its own methods aside), no `construct` into it.
pub fn skipped_drops(hir: &Hir, table: &TypeTable, res: &Resolution, placements: &mut Placements) {
    let d = Drops::new(hir, table, res);
    let whole = whole_functions(hir, table, res, placements, &d);
    let adders = foreign_adders(hir, table, res, placements.staying());
    for (_, body) in hir.bodies() {
        let Some(placed) = placements.body(&body.owner) else { continue };
        let decision: HashMap<ExprId, &Decision> =
            placed.sites.iter().map(|s| (s.call, &s.decision)).collect();
        let mut skipped = Vec::new();
        let mut sites = Vec::new();
        for s in (0..body.stmts.len() as u32).map(StmtId) {
            let hir::StmtKind::Let { local: Some(x), init: Some(init) } = body.stmt(s).kind else {
                continue;
            };
            let mut held = Vec::new();
            let built = built_in_region(&d, body, &decision, &whole, init, None, &mut held);
            if !built || replaced(&d, body, x) {
                continue;
            }
            let ty = &body.local(x).ty;
            let region = held.first().and_then(|e| match decision.get(e) {
                Some(Decision::Region { block, .. }) => Some(*block),
                _ => None,
            });
            // A container of values that own memory, `List<String>`, when everything put
            // into it was built whole in the same region (T-0145).
            let filled = |block| {
                let staying = placements.staying();
                let cx = Contents {
                    d: &d,
                    body,
                    decision: &decision,
                    whole: &whole,
                    adders: &adders,
                    staying,
                    block,
                };
                cx.whole_inside(x)
            };
            if d.releasable(ty) || (d.holder(ty) && region.is_some_and(filled)) {
                skipped.push(x);
                sites.extend(held);
            }
        }
        if !skipped.is_empty() {
            placements.mark_skipped(&body.owner, skipped, &sites);
        }
    }
}

/// Whether `e` builds everything it owns in one region, the same for every site in it, and
/// which sites those are.
fn built_in_region(
    d: &Drops<'_>,
    body: &Body,
    decision: &HashMap<ExprId, &Decision>,
    whole: &HashSet<BodyOwner>,
    e: ExprId,
    region: Option<BlockId>,
    held: &mut Vec<ExprId>,
) -> bool {
    match &body.expr(e).kind {
        hir::ExprKind::Call { callee, .. } => {
            let Some(Decision::Region { block, .. }) = decision.get(&e) else { return false };
            if region.is_some_and(|r| r != *block) {
                return false;
            }
            let Some((_, owner)) = callee_owner(d.res, callee) else { return false };
            held.push(e);
            whole.contains(&owner)
        }
        hir::ExprKind::StructLit { fields, .. } => {
            let region = region.or_else(|| {
                fields.iter().flatten().find_map(|f| match decision.get(f) {
                    Some(Decision::Region { block, .. }) => Some(*block),
                    _ => None,
                })
            });
            let Some(region) = region else { return false };
            let types = match body.ty(e) {
                Ty::Named { sym, args } => d
                    .table
                    .struct_sig(*sym)
                    .map(|s| d.table.field_types(*sym, args, &s.fields))
                    .unwrap_or_default(),
                _ => return false,
            };
            fields.iter().zip(types).all(|(f, t)| match f {
                // A field left to its default is built on the heap (a field's default).
                None => !d.owns(&t),
                Some(f) if !d.owns(body.ty(*f)) => true,
                Some(f) => built_in_region(d, body, decision, whole, *f, Some(region), held),
            })
        }
        _ => false,
    }
}

/// Whether anything in `body` may replace `x`, or an owning part of it, after its `let`:
/// an assignment, a `&mut` handed on, or `construct`, `destroy` or `move_out` on it. The
/// receiver of a method is not handed on: a container's methods keep its memory where it
/// was placed (§14.6), and a struct's own are [`Drops::sealed`]. Neither is a `for` over a
/// container's elements, which own nothing.
fn replaced(d: &Drops<'_>, body: &Body, x: LocalId) -> bool {
    // The place each use of `x` is the root of, and what holds that place.
    let mut parent: HashMap<ExprId, ExprId> = HashMap::new();
    for i in 0..body.exprs.len() as u32 {
        let e = ExprId(i);
        for c in children(body, e) {
            parent.insert(c, e);
        }
    }
    let mut stmt_of: HashMap<ExprId, StmtId> = HashMap::new();
    for s in (0..body.stmts.len() as u32).map(StmtId) {
        match &body.stmt(s).kind {
            hir::StmtKind::Assign { target, .. } => {
                stmt_of.insert(*target, s);
            }
            hir::StmtKind::For { subject, .. } => {
                stmt_of.insert(*subject, s);
            }
            _ => {}
        }
    }
    let uses = (0..body.exprs.len() as u32)
        .map(ExprId)
        .filter(|e| matches!(body.expr(*e).kind, hir::ExprKind::Local(l) if l == x));
    for u in uses {
        // Climb the place: `x.a`, `x.a[i]`, `*x`.
        let mut top = u;
        while let Some(&p) = parent.get(&top) {
            let place = match body.expr(p).kind {
                hir::ExprKind::Field { base, .. }
                | hir::ExprKind::TupleIndex { base, .. }
                | hir::ExprKind::Index { base, .. } => base == top,
                hir::ExprKind::Deref(_) => true,
                _ => false,
            };
            if !place {
                break;
            }
            top = p;
        }
        if !d.owns(body.ty(top)) && top != u {
            continue;
        }
        if let Some(s) = stmt_of.get(&top) {
            match body.stmt(*s).kind {
                hir::StmtKind::Assign { .. } => return true,
                hir::StmtKind::For { .. } => continue,
                _ => {}
            }
        }
        let Some(&p) = parent.get(&top) else { continue };
        match &body.expr(p).kind {
            hir::ExprKind::Ref { mutable: true, .. } => {
                // `for e in &mut xs` writes elements that own nothing.
                let walked = stmt_of
                    .get(&p)
                    .is_some_and(|s| matches!(body.stmt(*s).kind, hir::StmtKind::For { .. }));
                if !(walked && d.leaf(body.ty(top))) {
                    return true;
                }
            }
            hir::ExprKind::Builtin { name, args } | hir::ExprKind::Intrinsic { name, args }
                if matches!(name.as_str(), "construct" | "destroy" | "move_out")
                    && args.first() == Some(&top) =>
            {
                return true
            }
            _ => {}
        }
    }
    false
}

/// The expressions directly under `e`.
fn children(body: &Body, e: ExprId) -> Vec<ExprId> {
    use hir::ExprKind as K;
    match &body.expr(e).kind {
        K::Field { base, .. }
        | K::TupleIndex { base, .. }
        | K::Deref(base)
        | K::Len(base)
        | K::DataPtr(base)
        | K::Move(base)
        | K::Ref { place: base, .. }
        | K::Unary { operand: base, .. }
        | K::Cast { operand: base, .. } => vec![*base],
        K::Index { base, index } => vec![*base, *index],
        K::SliceRange { base, start, end } => {
            [Some(base), start.as_ref(), end.as_ref()].into_iter().flatten().copied().collect()
        }
        K::Binary { lhs, rhs, .. } => vec![*lhs, *rhs],
        K::Range { start, end, .. } => [start, end].into_iter().flatten().copied().collect(),
        K::StructLit { fields, .. } | K::UnionVariant { payload: fields, .. } => {
            fields.iter().flatten().copied().collect()
        }
        K::Tuple(parts)
        | K::ArrayLit(parts)
        | K::Builtin { args: parts, .. }
        | K::Intrinsic { args: parts, .. }
        | K::Formatted { args: parts, .. } => parts.clone(),
        K::Call { callee, args } => {
            let mut out = match callee {
                Callee::Method { recv, .. } | Callee::BaseMethod { recv, .. } => vec![*recv],
                Callee::Value(v) => vec![*v],
                _ => Vec::new(),
            };
            out.extend(args.iter().copied());
            out
        }
        K::If { cond, .. } => vec![*cond],
        K::IfLet { operand, .. } => vec![*operand],
        K::Switch { subject, arms, .. } => {
            let mut out = vec![*subject];
            out.extend(arms.iter().flat_map(|a| a.values.iter().copied()));
            out
        }
        K::Guarded { init, value, .. } => vec![*init, *value],
        K::Or { operand, clause } => match clause {
            hir::OrClause::Value(v) => vec![*operand, *v],
            _ => vec![*operand],
        },
        K::Return(Some(v)) => vec![*v],
        _ => Vec::new(),
    }
}

/// The functions that build everything their value owns in the placement they are given,
/// so a value from one is wholly where it was placed:
///
/// - a container's constructor that stores `#placement` ([`Drops::leaf`]'s contract);
/// - a struct's constructor, for a struct with no drop of its own and no base, that sets
///   each owning field, in its outermost block and wherever else it sets it, to a site
///   built in its placement by a function of this kind;
/// - any other function whose every `return` hands back something that owns nothing, or
///   such a site, or a local `let` bound to one and never replaced.
///
/// The smallest set closed under these rules, so a function reaching itself is not in it.
fn whole_functions(
    hir: &Hir,
    table: &TypeTable,
    res: &Resolution,
    placements: &Placements,
    d: &Drops<'_>,
) -> HashSet<BodyOwner> {
    let mut whole: HashSet<BodyOwner> = HashSet::new();
    let candidates: Vec<&Body> = hir
        .bodies()
        .map(|(_, b)| b)
        // A `copy` takes a placement too (T-0140), but no call reaches it by name: a copy is
        // never built here, which keeps a copy's drop.
        .filter(|b| {
            (placements.takes_result(&b.owner) && !matches!(b.owner, BodyOwner::Copy(_)))
                || matches!(b.owner, BodyOwner::Ctor { .. })
        })
        .collect();
    loop {
        let before = whole.len();
        for body in &candidates {
            if whole.contains(&body.owner) {
                continue;
            }
            let decision: HashMap<ExprId, &Decision> = placements
                .body(&body.owner)
                .map(|p| p.sites.iter().map(|s| (s.call, &s.decision)).collect())
                .unwrap_or_default();
            let site = |e: ExprId| {
                matches!(decision.get(&e), Some(Decision::Result { .. }))
                    && match &body.expr(e).kind {
                        hir::ExprKind::Call { callee, .. } => callee_owner(res, callee)
                            .is_some_and(|(_, owner)| whole.contains(&owner)),
                        _ => false,
                    }
            };
            let ok = match &body.owner {
                BodyOwner::Ctor { owner, .. } => {
                    let Some(s) = table.struct_sig(*owner) else { continue };
                    if s.drop.is_some() {
                        // A container: its memory is what it took from `#placement`.
                        reads_placement(body)
                    } else if s.base.is_some() {
                        false
                    } else {
                        ctor_builds_fields(d, body, s, site)
                    }
                }
                _ => returns_whole(d, body, site),
            };
            if ok {
                whole.insert(body.owner.clone());
            }
        }
        if whole.len() == before {
            return whole;
        }
    }
}

fn reads_placement(body: &Body) -> bool {
    body.exprs
        .iter()
        .any(|e| matches!(&e.kind, hir::ExprKind::Intrinsic { name, .. } if name == "placement"))
}

/// Whether a constructor sets every owning field of its struct, in its outermost block,
/// and every time it sets one, to a site `site` accepts.
fn ctor_builds_fields(
    d: &Drops<'_>,
    body: &Body,
    s: &dz_check::StructSig,
    site: impl Fn(ExprId) -> bool,
) -> bool {
    let field_of = |target: ExprId| match body.expr(target).kind {
        hir::ExprKind::Field { base, index } if self_field(body, target) => {
            let direct = match body.expr(base).kind {
                hir::ExprKind::Local(l) => body.local(l).name == "self",
                hir::ExprKind::Deref(inner) => matches!(body.expr(inner).kind,
                    hir::ExprKind::Local(l) if body.local(l).name == "self"),
                _ => false,
            };
            direct.then_some(index)
        }
        _ => None,
    };
    let mut set_at_top: HashSet<usize> = HashSet::new();
    let top: HashSet<StmtId> = body.block(body.root).stmts.iter().copied().collect();
    for st in (0..body.stmts.len() as u32).map(StmtId) {
        let hir::StmtKind::Assign { target, value, op, .. } = body.stmt(st).kind else { continue };
        if !d.owns(body.ty(target)) {
            continue;
        }
        let Some(index) = field_of(target) else {
            // An owning part of a field, or something reached through one.
            if self_field(body, target) {
                return false;
            }
            continue;
        };
        if op != hir::AssignOp::Assign || !site(value) {
            return false;
        }
        if top.contains(&st) {
            set_at_top.insert(index);
        }
    }
    // A field never set keeps its default, which is built on the heap.
    let types: Vec<&Ty> = s.fields.iter().map(|f| &f.ty).collect();
    types.iter().enumerate().all(|(i, t)| !d.owns(t) || set_at_top.contains(&i))
        && !replaces_self_by_ref(d, body)
}

/// Whether a constructor hands an owning field of `self` on as `&mut`, or `construct`s into
/// one: the field may then hold anything.
fn replaces_self_by_ref(d: &Drops<'_>, body: &Body) -> bool {
    (0..body.exprs.len() as u32).map(ExprId).any(|e| match &body.expr(e).kind {
        hir::ExprKind::Ref { mutable: true, place, .. } => {
            self_field(body, *place) && d.owns(body.ty(*place))
        }
        hir::ExprKind::Builtin { name, args } | hir::ExprKind::Intrinsic { name, args } => {
            matches!(name.as_str(), "construct" | "move_out")
                && args.first().is_some_and(|a| self_field(body, *a))
        }
        _ => false,
    })
}

/// Whether every `return` in `body` hands back what owns nothing, or a site `site` accepts,
/// or a local bound to one and never replaced.
fn returns_whole(d: &Drops<'_>, body: &Body, site: impl Fn(ExprId) -> bool) -> bool {
    let peel = |mut e: ExprId| loop {
        match body.expr(e).kind {
            hir::ExprKind::Move(inner)
            | hir::ExprKind::Cast { operand: inner, explicit: false } => e = inner,
            _ => return e,
        }
    };
    let bound = |l: LocalId| {
        (0..body.stmts.len() as u32).map(StmtId).any(|s| {
            matches!(body.stmt(s).kind, hir::StmtKind::Let { local: Some(x), init: Some(i) }
                if x == l && site(i))
        }) && !replaced(d, body, l)
    };
    body.exprs.iter().all(|node| {
        let hir::ExprKind::Return(Some(v)) = node.kind else { return true };
        if !d.owns(body.ty(v)) {
            return true;
        }
        // What was put into a container of owning values on its way out is not followed
        // across the call (T-0145): such a result is never whole.
        if d.holder(body.ty(v).peeled()) {
            return false;
        }
        let v = peel(v);
        match body.expr(v).kind {
            // `return none` from a `-> String?` hands back nothing at all.
            hir::ExprKind::NoneOpt => true,
            hir::ExprKind::Local(l) => bound(l),
            _ => site(v),
        }
    })
}

/// The methods that may put into `self` a value they were not given (T-0145): a
/// `construct` into memory reached from `self`, or an assignment to an owning field of it,
/// of anything but a parameter moved in or a value moved out of memory with `move_out`
/// (which moves an element the container already had, as growing does); a call on `self`
/// or a field of it to such a method, or to a trait's `&mut self` method; or handing a
/// stored parameter of a method anything else. `List::resize`, which builds default
/// elements, and `Extend::extend`, which copies, are among them; `List::push` and
/// `HashMap::insert` are not.
fn foreign_adders(
    hir: &Hir,
    table: &TypeTable,
    res: &Resolution,
    staying: &crate::placement::Staying,
) -> HashSet<BodyOwner> {
    let bodies: Vec<&Body> = hir
        .bodies()
        .map(|(_, b)| b)
        .filter(|b| matches!(b.owner, BodyOwner::Method { .. } | BodyOwner::ImplMethod { .. }))
        .collect();
    let mut adders: HashSet<BodyOwner> = HashSet::new();
    loop {
        let before = adders.len();
        for body in &bodies {
            if !adders.contains(&body.owner) && adds_foreign(table, res, staying, body, &adders) {
                adders.insert(body.owner.clone());
            }
        }
        if adders.len() == before {
            return adders;
        }
    }
}

fn adds_foreign(
    table: &TypeTable,
    res: &Resolution,
    staying: &crate::placement::Staying,
    body: &Body,
    adders: &HashSet<BodyOwner>,
) -> bool {
    let in_self = |mut e: ExprId| loop {
        match body.expr(e).kind {
            hir::ExprKind::Local(l) => return body.local(l).name == "self",
            hir::ExprKind::Field { base, .. }
            | hir::ExprKind::TupleIndex { base, .. }
            | hir::ExprKind::Index { base, .. }
            | hir::ExprKind::Deref(base) => e = base,
            _ => return false,
        }
    };
    // A parameter moved in, a value moved out of memory, or one that owns nothing: not a
    // new owner.
    let given = |e: ExprId| match &body.expr(e).kind {
        hir::ExprKind::Move(inner) => {
            matches!(body.expr(*inner).kind, hir::ExprKind::Local(l) if body.params.contains(&l))
        }
        hir::ExprKind::Builtin { name, .. } | hir::ExprKind::Intrinsic { name, .. }
            if name == "move_out" =>
        {
            true
        }
        _ => !crate::storing::owns(table, body.ty(e)),
    };
    let assigned = (0..body.stmts.len() as u32).map(StmtId).any(|s| match body.stmt(s).kind {
        hir::StmtKind::Assign { target, value, .. } => {
            in_self(target) && crate::storing::owns(table, body.ty(target)) && !given(value)
        }
        _ => false,
    });
    assigned
        || body.exprs.iter().any(|node| match &node.kind {
            hir::ExprKind::Builtin { name, args } | hir::ExprKind::Intrinsic { name, args }
                if name == "construct" =>
            {
                args.first().is_some_and(|a| in_self(*a))
                    && args.get(1).map_or(true, |v| !given(*v))
            }
            hir::ExprKind::Call { callee, args } => {
                let (Callee::Method { recv, .. } | Callee::BaseMethod { recv, .. }) = callee else {
                    return false;
                };
                if !in_self(*recv) {
                    return false;
                }
                let Some((_, owner)) = callee_owner(res, callee) else { return true };
                if matches!(owner, BodyOwner::TraitDefaults { .. }) {
                    // A trait's method on `self`: which body runs is not followed.
                    return mutates(table, callee);
                }
                adders.contains(&owner)
                    || args
                        .iter()
                        .enumerate()
                        .any(|(i, a)| staying.param_stored(&owner, i) && !given(*a))
            }
            _ => false,
        })
}

/// Whether `callee` is a method that takes `&mut self`; an unknown one is taken to.
fn mutates(table: &TypeTable, callee: &Callee) -> bool {
    let (Callee::Method { owner, name, .. } | Callee::BaseMethod { owner, name, .. }) = callee
    else {
        return false;
    };
    table
        .methods_sig(*owner)
        .and_then(|s| s.methods.get(name))
        .map_or(true, |sig| sig.takes_self == Some(true))
}

/// Whether a value of `ty` can write what it was taken from: a `&mut`, a `^mut`, a writable
/// slice, or a view struct (an iterator over `&mut` elements is one).
fn writes_through(table: &TypeTable, ty: &Ty) -> bool {
    match ty {
        Ty::Ref { mutable, .. } | Ty::Ptr { mutable, .. } | Ty::Slice { mutable, .. } => *mutable,
        Ty::OwnPtr { .. } => true,
        Ty::Optional(inner) | Ty::Fallible(inner) => writes_through(table, inner),
        Ty::Tuple(parts) => parts.iter().any(|t| writes_through(table, t)),
        Ty::Named { sym, .. } => table.is_view_struct(*sym),
        _ => false,
    }
}

/// What a body puts into one container local, asked of a [`Drops::holder`] (T-0145).
struct Contents<'c, 'a> {
    d: &'c Drops<'a>,
    body: &'c Body,
    decision: &'c HashMap<ExprId, &'c Decision>,
    whole: &'c HashSet<BodyOwner>,
    adders: &'c HashSet<BodyOwner>,
    staying: &'c crate::placement::Staying,
    /// The block whose region the container is in.
    block: BlockId,
}

impl Contents<'_, '_> {
    /// Whether everything ever in the container in `x` was built whole in its region: each
    /// `&mut self` method called on it is one of its type's own, adds nothing it was not
    /// given ([`foreign_adders`]), hands back nothing that can write an element (`xs[i] =`,
    /// `get_or_insert`, `iter_mut`), and is given, for each value it stores, a site built
    /// whole in the container's region, directly or through a `let` nothing replaces.
    fn whole_inside(&self, x: LocalId) -> bool {
        let body = self.body;
        let mut parent: HashMap<ExprId, ExprId> = HashMap::new();
        for i in 0..body.exprs.len() as u32 {
            for c in crate::storing::children(body, ExprId(i)) {
                parent.insert(c, ExprId(i));
            }
        }
        let uses = (0..body.exprs.len() as u32)
            .map(ExprId)
            .filter(|e| matches!(body.expr(*e).kind, hir::ExprKind::Local(l) if l == x));
        for u in uses {
            let mut top = u;
            while let Some(&p) = parent.get(&top) {
                let place = matches!(body.expr(p).kind,
                    hir::ExprKind::Field { base, .. } | hir::ExprKind::TupleIndex { base, .. }
                    if base == top);
                if !place {
                    break;
                }
                top = p;
            }
            let Some(&p) = parent.get(&top) else { continue };
            let hir::ExprKind::Call { callee, args } = &body.expr(p).kind else { continue };
            let (Callee::Method { recv, .. } | Callee::BaseMethod { recv, .. }) = callee else {
                continue;
            };
            if *recv != top || !mutates(self.d.table, callee) {
                continue;
            }
            let Some((_, owner)) = callee_owner(self.d.res, callee) else { return false };
            if !matches!(owner, BodyOwner::Method { .. })
                || self.adders.contains(&owner)
                || writes_through(self.d.table, body.ty(p))
            {
                return false;
            }
            for (i, a) in args.iter().enumerate() {
                if self.staying.param_stored(&owner, i) && !self.built_here(*a) {
                    return false;
                }
            }
        }
        true
    }

    /// Whether `e`, a value stored into the container, was built whole in its region: a
    /// site there, a struct literal of them, or a local a `let` bound to one, moved in and
    /// never replaced.
    fn built_here(&self, e: ExprId) -> bool {
        let body = self.body;
        if !self.d.owns(body.ty(e)) {
            return true;
        }
        // A local passed without `move` is copied, and a copy is built on the heap (T-0140).
        let (e, moved) = match body.expr(e).kind {
            hir::ExprKind::Move(inner) => (inner, true),
            _ => (e, false),
        };
        let mut held = Vec::new();
        if let hir::ExprKind::Local(l) = body.expr(e).kind {
            if !moved {
                return false;
            }
            let bound =
                (0..body.stmts.len() as u32).map(StmtId).find_map(|s| match body.stmt(s).kind {
                    hir::StmtKind::Let { local: Some(x), init: Some(i) } if x == l => Some(i),
                    _ => None,
                });
            return bound.is_some_and(|i| {
                built_in_region(
                    self.d,
                    body,
                    self.decision,
                    self.whole,
                    i,
                    Some(self.block),
                    &mut held,
                ) && !replaced(self.d, body, l)
            });
        }
        built_in_region(self.d, body, self.decision, self.whole, e, Some(self.block), &mut held)
    }
}
