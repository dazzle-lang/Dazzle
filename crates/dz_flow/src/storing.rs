//! What a container does with what it is given (spec §14.6, T-0145): which by-value
//! parameters a method stores into `self`, and which containers in a body keep everything
//! stored into them.
//!
//! A value passed to `names.push(…)` lives as long as `names`, so it may be built where
//! `names` lives: placement follows it there ([`crate::placement`]), and the region check
//! follows it into the container and back out ([`crate::ties::analyse_regions`]). The
//! facts both read are worked out here, from the methods' bodies, since nothing in a
//! signature says that `push` keeps `v`: it moves it into memory the list owns through
//! `unsafe { construct(m_data[m_len], move(v)) }`.

use std::collections::{HashMap, HashSet};

use dz_ast::TieStep;
use dz_check::TypeTable;
use dz_hir::ty::Ty;
use dz_hir::{self as hir, BlockId, Body, BodyOwner, Callee, ExprId, Hir, LocalId, StmtId};
use dz_project::ModuleId;
use dz_resolve::Resolution;

use crate::placement::{callee_owner, owner_module, Staying};

/// Each method's by-value parameters it stores into `self`, with whether every call keeps
/// the value (stores it on every path, and drops none of it).
///
/// A parameter is stored when each use of it borrows it, copies it, or moves it into `self`:
/// `construct` into memory reached from a field of `self`, an assignment to a field of
/// `self`, or a stored parameter of a method called on `self` or a field of it (so
/// `HashMap::insert` passing its key to `replace`, and `HashSet::insert` to its map's
/// `replace`, are followed). It may also be dropped: `replace` drops the key it already had.
/// A parameter that stays in the call ([`Staying`]) counts as dropped.
///
/// It is **kept** when the move into `self` is a statement of the method's outermost block,
/// reached through nothing but `unsafe` and plain blocks, after no `return`, and what it is
/// handed to keeps it too: `List::push` and `List::insert` keep; `HashMap::insert` and
/// `get_or_insert`, which may drop the value they were given, do not.
///
/// Claims start at every by-value parameter of every `&mut self` method and only shrink,
/// so the fixed point is reached.
pub(crate) fn storing_params(
    hir: &Hir,
    table: &TypeTable,
    res: &Resolution,
    staying: &Staying,
) -> HashMap<BodyOwner, (ModuleId, Vec<(usize, bool)>)> {
    let mut bodies: HashMap<BodyOwner, &Body> = HashMap::new();
    let mut claims: HashMap<BodyOwner, Vec<usize>> = HashMap::new();
    for (_, body) in hir.bodies() {
        let BodyOwner::Method { owner, name } = &body.owner else { continue };
        if res.symbol(*owner).kind == dz_resolve::SymbolKind::Trait {
            continue;
        }
        let Some(sig) = table.methods_sig(*owner).and_then(|s| s.methods.get(name)) else {
            continue;
        };
        if sig.dispatch != dz_ast::Dispatch::Static || sig.takes_self != Some(true) {
            continue;
        }
        let params: Vec<usize> = (0..sig.params.len())
            .filter(|i| {
                body.params.get(i + 1).is_some_and(|l| {
                    !matches!(
                        body.local(*l).ty,
                        Ty::Ref { .. } | Ty::Ptr { .. } | Ty::OwnPtr { .. }
                    )
                })
            })
            .collect();
        if !params.is_empty() {
            claims.insert(body.owner.clone(), params);
            bodies.insert(body.owner.clone(), body);
        }
    }

    // Which parameters are stored, and at which expression (the anchor `kept` asks about).
    let mut anchors: HashMap<(BodyOwner, usize), Vec<Anchor>> = HashMap::new();
    loop {
        let before = claims.clone();
        anchors.clear();
        for (owner, params) in &mut claims {
            let body = bodies[owner];
            let walk = Walk::new(body);
            params.retain(|i| {
                match walk.stored(res, table, staying, &before, body.params[i + 1]) {
                    Some(found) => {
                        anchors.insert((owner.clone(), *i), found);
                        true
                    }
                    None => false,
                }
            });
        }
        claims.retain(|_, p| !p.is_empty());
        if claims == before {
            break;
        }
    }

    // Which of those are kept, again to a fixed point: a parameter handed on is kept only
    // when the callee keeps it.
    let mut kept: HashSet<(BodyOwner, usize)> = anchors
        .keys()
        .filter(|(o, i)| claims.get(o).is_some_and(|p| p.contains(i)))
        .cloned()
        .collect();
    loop {
        let before = kept.clone();
        kept.retain(|(owner, i)| {
            let body = bodies[owner];
            anchors[&(owner.clone(), *i)].iter().any(|a| unconditional(body, a) && a.kept(&before))
        });
        if kept == before {
            break;
        }
    }

    claims
        .into_iter()
        .map(|(owner, params)| {
            let module = owner_module(table, res, &owner);
            let params =
                params.into_iter().map(|i| (i, kept.contains(&(owner.clone(), i)))).collect();
            (owner, (module, params))
        })
        .collect()
}

/// Where a parameter is moved into `self`.
#[derive(Clone, Debug)]
enum Anchor {
    /// `construct(self…, move(p))`, or `self.f = move(p)`: the expression, or the
    /// statement's value.
    Here(ExprId),
    /// Handed to parameter `index` of the method `owner`, called on `self` or a field of it.
    Handed { call: ExprId, owner: BodyOwner, index: usize },
}

impl Anchor {
    fn expr(&self) -> ExprId {
        match self {
            Anchor::Here(e) => *e,
            Anchor::Handed { call, .. } => *call,
        }
    }

    fn kept(&self, kept: &HashSet<(BodyOwner, usize)>) -> bool {
        match self {
            Anchor::Here(_) => true,
            Anchor::Handed { owner, index, .. } => kept.contains(&(owner.clone(), *index)),
        }
    }
}

/// Whether `anchor` runs on every call of `body`: it is in a statement of the outermost
/// block, through nothing but plain and `unsafe` blocks, and no earlier statement returns.
fn unconditional(body: &Body, anchor: &Anchor) -> bool {
    let target = anchor.expr();
    for s in &body.block(body.root).stmts {
        if direct(body, *s).contains(&target) {
            return true;
        }
        if returns(body, *s) {
            return false;
        }
    }
    false
}

/// The expressions a statement runs on every pass through it: its own, and those of the
/// statements of a plain or `unsafe` block it is.
fn direct(body: &Body, s: StmtId) -> Vec<ExprId> {
    match &body.stmt(s).kind {
        hir::StmtKind::Expr(e) => match body.expr(*e).kind {
            hir::ExprKind::Block(b) => {
                body.block(b).stmts.iter().flat_map(|s| direct(body, *s)).collect()
            }
            _ => vec![*e],
        },
        hir::StmtKind::Assign { value, .. } => vec![*value],
        _ => Vec::new(),
    }
}

/// Whether a statement may return, anywhere inside it.
fn returns(body: &Body, s: StmtId) -> bool {
    let mut exprs = Vec::new();
    stmt_exprs(body, s, &mut exprs);
    exprs.iter().any(|e| matches!(body.expr(*e).kind, hir::ExprKind::Return(_)))
}

fn stmt_exprs(body: &Body, s: StmtId, out: &mut Vec<ExprId>) {
    match &body.stmt(s).kind {
        hir::StmtKind::Let { init, .. } => out.extend(init.iter().flat_map(|e| subtree(body, *e))),
        hir::StmtKind::LetTuple { init, .. } | hir::StmtKind::Expr(init) => {
            out.extend(subtree(body, *init))
        }
        hir::StmtKind::Assign { target, value, .. } => {
            out.extend(subtree(body, *target));
            out.extend(subtree(body, *value));
        }
        hir::StmtKind::While { cond, body: b, .. } => {
            out.extend(subtree(body, *cond));
            block_exprs(body, *b, out);
        }
        hir::StmtKind::For { subject, body: b, .. } => {
            out.extend(subtree(body, *subject));
            block_exprs(body, *b, out);
        }
        hir::StmtKind::Defer(inner) => stmt_exprs(body, *inner, out),
        hir::StmtKind::Item(_) | hir::StmtKind::Error => {}
    }
}

fn block_exprs(body: &Body, b: BlockId, out: &mut Vec<ExprId>) {
    for s in &body.block(b).stmts {
        stmt_exprs(body, *s, out);
    }
}

/// `e` and everything under it, blocks included.
fn subtree(body: &Body, e: ExprId) -> Vec<ExprId> {
    let mut out = vec![e];
    let mut i = 0;
    while i < out.len() {
        let x = out[i];
        out.extend(children(body, x));
        let mut inner = Vec::new();
        for b in blocks_of(body, x) {
            block_exprs(body, b, &mut inner);
        }
        out.extend(inner);
        i += 1;
    }
    out
}

fn blocks_of(body: &Body, e: ExprId) -> Vec<BlockId> {
    match &body.expr(e).kind {
        hir::ExprKind::Block(b) => vec![*b],
        hir::ExprKind::If { then, else_branch, .. }
        | hir::ExprKind::IfLet { then, else_branch, .. } => {
            let mut v = vec![*then];
            v.extend(else_branch.iter().copied());
            v
        }
        hir::ExprKind::Switch { arms, default, .. } => {
            let mut v: Vec<BlockId> = arms.iter().map(|a| a.body).collect();
            v.extend(default.iter().copied());
            v
        }
        hir::ExprKind::Formatted { body: b, .. } => vec![*b],
        hir::ExprKind::Or { clause: hir::OrClause::Block { body: b, .. }, .. } => vec![*b],
        _ => Vec::new(),
    }
}

/// The expressions directly under `e`, not counting the blocks it holds.
pub(crate) fn children(body: &Body, e: ExprId) -> Vec<ExprId> {
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

/// One body's expressions, each with the expression or statement it is directly under.
struct Walk<'b> {
    body: &'b Body,
    parent: HashMap<ExprId, ExprId>,
    stmt: HashMap<ExprId, StmtId>,
}

impl<'b> Walk<'b> {
    fn new(body: &'b Body) -> Self {
        let mut parent = HashMap::new();
        for i in 0..body.exprs.len() as u32 {
            for c in children(body, ExprId(i)) {
                parent.insert(c, ExprId(i));
            }
        }
        let mut stmt = HashMap::new();
        for s in (0..body.stmts.len() as u32).map(StmtId) {
            match &body.stmt(s).kind {
                hir::StmtKind::Let { init: Some(e), .. }
                | hir::StmtKind::LetTuple { init: e, .. }
                | hir::StmtKind::Expr(e) => {
                    stmt.insert(*e, s);
                }
                hir::StmtKind::Assign { target, value, .. } => {
                    stmt.insert(*target, s);
                    stmt.insert(*value, s);
                }
                hir::StmtKind::While { cond: e, .. } | hir::StmtKind::For { subject: e, .. } => {
                    stmt.insert(*e, s);
                }
                _ => {}
            }
        }
        Walk { body, parent, stmt }
    }

    /// Whether `e` is `self`, or a place reached from it: a field, an element through a
    /// pointer field, a dereference.
    fn in_self(&self, e: ExprId) -> bool {
        match self.body.expr(e).kind {
            hir::ExprKind::Local(l) => self.body.local(l).name == "self",
            hir::ExprKind::Field { base, .. }
            | hir::ExprKind::TupleIndex { base, .. }
            | hir::ExprKind::Index { base, .. }
            | hir::ExprKind::Deref(base) => self.in_self(base),
            _ => false,
        }
    }

    /// Where `p` is moved into `self`, when every use of it borrows, copies, drops or moves
    /// it there; `None` when one takes it anywhere else.
    fn stored(
        &self,
        res: &Resolution,
        table: &TypeTable,
        staying: &Staying,
        claims: &HashMap<BodyOwner, Vec<usize>>,
        p: LocalId,
    ) -> Option<Vec<Anchor>> {
        let copyable = table.copyable(&self.body.local(p).ty);
        let mut anchors = Vec::new();
        let uses = (0..self.body.exprs.len() as u32)
            .map(ExprId)
            .filter(|e| matches!(self.body.expr(*e).kind, hir::ExprKind::Local(l) if l == p));
        for u in uses {
            // A field or element of `p` read in place: what is done with it decides.
            let mut top = u;
            while let Some(&up) = self.parent.get(&top) {
                let place = matches!(self.body.expr(up).kind,
                    hir::ExprKind::Field { base, .. }
                    | hir::ExprKind::TupleIndex { base, .. }
                    | hir::ExprKind::Index { base, .. } if base == top);
                if !place {
                    break;
                }
                top = up;
            }
            let Some(&up) = self.parent.get(&top) else {
                // A statement's own expression. Assigned to, `p` holds something new;
                // anything else that is not a copy takes it by value.
                let assigned = self.stmt.get(&top).is_some_and(|s| {
                    matches!(self.body.stmt(*s).kind, hir::StmtKind::Assign { target, .. } if target == top)
                });
                if assigned || !copyable {
                    return None;
                }
                continue;
            };
            match &self.body.expr(up).kind {
                hir::ExprKind::Ref { .. } => {}
                hir::ExprKind::Call { callee: Callee::Method { recv, .. }, .. } if *recv == top => {
                }
                hir::ExprKind::Builtin { name, args } | hir::ExprKind::Intrinsic { name, args }
                    if name == "destroy" && args.first() == Some(&top) => {}
                hir::ExprKind::Move(_) if top == u => {
                    anchors.push(self.moved_into_self(res, staying, claims, up)?);
                }
                _ if copyable => {}
                _ => return None,
            }
        }
        Some(anchors)
    }

    /// Where `move(p)`, the expression `m`, goes: into `self`, or `None`.
    fn moved_into_self(
        &self,
        res: &Resolution,
        staying: &Staying,
        claims: &HashMap<BodyOwner, Vec<usize>>,
        m: ExprId,
    ) -> Option<Anchor> {
        let Some(&up) = self.parent.get(&m) else {
            let s = self.stmt.get(&m)?;
            return match self.body.stmt(*s).kind {
                hir::StmtKind::Assign { target, value, op: hir::AssignOp::Assign, .. }
                    if value == m && crate::placement::self_field(self.body, target) =>
                {
                    Some(Anchor::Here(m))
                }
                _ => None,
            };
        };
        match &self.body.expr(up).kind {
            hir::ExprKind::Builtin { name, args } | hir::ExprKind::Intrinsic { name, args }
                if name == "construct"
                    && args.get(1) == Some(&m)
                    && args.first().is_some_and(|a| self.in_self(*a)) =>
            {
                Some(Anchor::Here(up))
            }
            hir::ExprKind::Call { callee, args } => {
                let index = args.iter().position(|a| *a == m)?;
                let Callee::Method { recv, .. } = callee else { return None };
                if !self.in_self(*recv) {
                    return None;
                }
                let (_, owner) = callee_owner(res, callee)?;
                if claims.get(&owner).is_some_and(|p| p.contains(&index)) {
                    Some(Anchor::Handed { call: up, owner, index })
                } else if staying.param_stays(&owner, index) {
                    // Gone when the call returns: dropped, as far as `self` cares. Never
                    // kept, so an anchor no `kept` set holds.
                    Some(Anchor::Handed { call: up, owner, index: usize::MAX })
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

/// Whether the container in local `l` keeps everything stored into it, for as long as `l`
/// lives (T-0145): nothing in `body` takes an element out of it or drops one. Every use is
/// a borrow, a read, or a method that changes at most where its elements are (`kills
/// self@items` and nothing under `[*]`) and hands back nothing that owns memory. A `&mut`
/// lent, an assignment to it or into it, a move of it, or a method that may drop or return
/// an element (`pop`, `clear`, `HashMap::insert`, which may drop the value it replaces)
/// says no.
///
/// This is not a safety rule: the region check follows an element taken out of a container
/// wherever it goes. It bounds memory. A value built on every pass of a loop may go into a
/// region the loop does not end only when the container it is stored into keeps it, so the
/// region grows no faster than the container does (T-0117's loop rule).
pub(crate) fn keeps(body: &Body, table: &TypeTable, l: LocalId) -> bool {
    let walk = Walk::new(body);
    let uses = (0..body.exprs.len() as u32)
        .map(ExprId)
        .filter(|e| matches!(body.expr(*e).kind, hir::ExprKind::Local(x) if x == l));
    for u in uses {
        let mut top = u;
        while let Some(&up) = walk.parent.get(&top) {
            let place = matches!(body.expr(up).kind,
                hir::ExprKind::Field { base, .. }
                | hir::ExprKind::TupleIndex { base, .. }
                | hir::ExprKind::Index { base, .. } if base == top);
            if !place {
                break;
            }
            top = up;
        }
        if walk.stmt.get(&top).is_some_and(
            |s| matches!(body.stmt(*s).kind, hir::StmtKind::Assign { target, .. } if target == top),
        ) {
            return false;
        }
        let Some(&up) = walk.parent.get(&top) else { continue };
        match &body.expr(up).kind {
            hir::ExprKind::Ref { mutable: true, .. } | hir::ExprKind::Move(_) => return false,
            hir::ExprKind::Builtin { name, args } | hir::ExprKind::Intrinsic { name, args }
                if args.first() == Some(&top)
                    && matches!(name.as_str(), "construct" | "destroy" | "move_out") =>
            {
                return false
            }
            hir::ExprKind::Call { callee, .. } => {
                let recv = match callee {
                    Callee::Method { recv, .. } | Callee::BaseMethod { recv, .. } => *recv,
                    _ => continue,
                };
                if recv != top {
                    continue;
                }
                if !keeps_through(table, callee, body.ty(up)) {
                    return false;
                }
            }
            _ => {}
        }
    }
    true
}

/// Whether a method called on a container leaves every element in it: it takes `&self`, or
/// it takes `&mut self`, kills at most `self@items` or `self@text` (where the elements are,
/// not the elements), and returns nothing that owns memory.
fn keeps_through(table: &TypeTable, callee: &Callee, ret: &Ty) -> bool {
    let (Callee::Method { owner, name, .. } | Callee::BaseMethod { owner, name, .. }) = callee
    else {
        return false;
    };
    let Some(sig) = table.methods_sig(*owner).and_then(|s| s.methods.get(name)) else {
        return false;
    };
    if sig.takes_self != Some(true) {
        return true;
    }
    if sig.kills_unknown || owns(table, ret) {
        return false;
    }
    sig.kills.iter().all(|path| {
        path.root.name == "self"
            && matches!(path.steps.as_slice(), [TieStep::Tie(t)] if t.name == "items" || t.name == "text")
    })
}

/// Whether a value of `ty` owns memory its drop gives back, an optional one included.
pub(crate) fn owns(table: &TypeTable, ty: &Ty) -> bool {
    match ty {
        Ty::Optional(inner) | Ty::Fallible(inner) => owns(table, inner),
        _ => table.needs_drop(ty),
    }
}
