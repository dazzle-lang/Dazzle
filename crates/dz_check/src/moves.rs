//! Values move by default (spec §6.4, D-0106).
//!
//! A by-value read of a place the function owns — a local, a by-value parameter, a field
//! of either — whose type is not plain data moves it, with nothing written. This pass
//! writes the `Move` the source no longer spells, around each such read, before any other
//! phase sees the tree: flow then finds every move where it always found one, placement
//! places by them (the condition T-0117 set, that codegen never turns a read into a move),
//! and codegen lowers each as it lowers a written one.
//!
//! Which reads are by value is the classification flow analysis makes (`value` against
//! `place_only` in `dz_flow`): the operand of `let`, an argument, a field of a literal, a
//! returned value; not a method's receiver, the operand of `&`, the base of a `.` or an
//! index, or anything a comparison or a formatting macro refers to. The two must agree,
//! and flow reports a by-value read of a non-plain owned place left unmoved, so a context
//! one of them forgets is a diagnostic rather than a silent copy.
//!
//! Left as they are:
//!
//! - **plain data**, which copies: a copy and a move of it are the same bytes;
//! - **a read written `a.clone()`**, which is the copy asked for ([`hir::Body::clones`]);
//! - **a place the function does not own** — through a reference, a pointer, an index or
//!   a `static` — which flow refuses to move out of (`DZ0402`);
//! - **`return x` of a whole local**, which C++ already moves, and which a written move
//!   would keep from being built in place: nothing runs after it to see the difference.

use dz_hir::ty::Ty;
use dz_hir::{self as hir, BlockId, ExprId, StmtId};

use crate::TypeTable;

/// Write a `Move` around every by-value read of an owned place whose type is not plain.
pub(crate) fn insert(body: &mut hir::Body, table: &TypeTable) {
    let mut w = Walk { body, table, wrap: Vec::new() };
    let root = w.body.root;
    w.block(root);
    // Wrapped last, so the walk above reads the tree as it was built.
    for e in std::mem::take(&mut w.wrap) {
        let node = w.body.exprs[e.index()].clone();
        let inner = ExprId(w.body.exprs.len() as u32);
        w.body.exprs.push(node);
        w.body.exprs[e.index()].kind = hir::ExprKind::Move(inner);
    }
}

struct Walk<'a> {
    body: &'a mut hir::Body,
    table: &'a TypeTable,
    /// The reads to wrap, in the order found.
    wrap: Vec<ExprId>,
}

impl Walk<'_> {
    fn block(&mut self, id: BlockId) {
        for s in self.body.block(id).stmts.clone() {
            self.stmt(s);
        }
    }

    fn stmt(&mut self, id: StmtId) {
        match self.body.stmt(id).kind.clone() {
            hir::StmtKind::Let { init, .. } => {
                if let Some(init) = init {
                    self.value(init);
                }
            }
            hir::StmtKind::LetTuple { init, .. } => self.value(init),
            hir::StmtKind::Assign { target, value, .. } => {
                self.value(value);
                self.read(target);
            }
            hir::StmtKind::Expr(e) => self.value(e),
            hir::StmtKind::While { cond, body, .. } => {
                self.read(cond);
                self.block(body);
            }
            hir::StmtKind::For { subject, body, shape, .. } => {
                // A user iterator is moved into the loop's own; an array or a slice is
                // walked in place, and a container through the iterator it hands out
                // (spec §10.4).
                if shape == hir::ForShape::Consuming
                    || matches!(
                        self.body.ty(subject).peeled(),
                        Ty::Named { .. } | Ty::Param(_) | Ty::Assoc(_)
                    )
                {
                    self.value(subject);
                } else {
                    self.read(subject);
                }
                self.block(body);
            }
            hir::StmtKind::Defer(inner) => self.stmt(inner),
            hir::StmtKind::Item(_) | hir::StmtKind::Error => {}
        }
    }

    /// An expression whose value is taken: walked, then moved if it is an owned place that
    /// is not plain.
    fn value(&mut self, e: ExprId) {
        self.read(e);
        if self.moves(e) {
            self.wrap.push(e);
        }
    }

    /// Whether a by-value read of `e` is a move.
    fn moves(&self, e: ExprId) -> bool {
        let node = self.body.expr(e);
        // A reference is never reset by a move: what it refers to is not the reader's.
        if matches!(node.ty, Ty::Ref { .. } | Ty::Error) || self.body.is_clone(e) {
            return false;
        }
        self.owned_place(e) && !self.table.plain(&node.ty)
    }

    /// A local this function owns, or a field of one reached without a pointer: what a
    /// move may reset (spec §6.4). `self` is a receiver, not owned.
    fn owned_place(&self, e: ExprId) -> bool {
        match &self.body.expr(e).kind {
            hir::ExprKind::Local(l) => self.body.local(*l).name != "self",
            hir::ExprKind::Field { base, .. } | hir::ExprKind::TupleIndex { base, .. } => {
                !matches!(self.body.ty(*base), Ty::Ptr { .. } | Ty::OwnPtr { .. } | Ty::Ref { .. })
                    && self.owned_place(*base)
            }
            _ => false,
        }
    }

    /// The parts of a place it is built from: `a.b[i].c` reads `i`, and refers to `a.b`.
    fn place_bases(&mut self, e: ExprId) {
        match self.body.expr(e).kind.clone() {
            hir::ExprKind::Field { base, .. } | hir::ExprKind::TupleIndex { base, .. } => {
                self.place_bases(base)
            }
            hir::ExprKind::Index { base, index } => {
                self.read(base);
                self.value(index);
            }
            hir::ExprKind::Deref(inner) => self.read(inner),
            _ => {}
        }
    }

    /// Walk an expression without taking its value, finding the by-value reads inside it.
    fn read(&mut self, e: ExprId) {
        match self.body.expr(e).kind.clone() {
            hir::ExprKind::Local(_)
            | hir::ExprKind::Field { .. }
            | hir::ExprKind::TupleIndex { .. }
            | hir::ExprKind::Index { .. }
            | hir::ExprKind::Deref(_) => self.place_bases(e),
            hir::ExprKind::Ref { place, .. } => self.read(place),
            // A move written out already moves; what it moves is walked for its parts.
            hir::ExprKind::Move(inner) => self.read(inner),
            hir::ExprKind::Unary { operand, .. } | hir::ExprKind::Cast { operand, .. } => {
                self.value(operand)
            }
            hir::ExprKind::Binary { lhs, rhs, .. } => {
                self.value(lhs);
                self.value(rhs);
            }
            hir::ExprKind::Range { start, end, .. } => {
                for x in [start, end].into_iter().flatten() {
                    self.value(x);
                }
            }
            hir::ExprKind::StructLit { fields, .. } => {
                for f in fields.into_iter().flatten() {
                    self.value(f);
                }
            }
            hir::ExprKind::UnionVariant { payload, .. } => {
                for f in payload.into_iter().flatten() {
                    self.value(f);
                }
            }
            hir::ExprKind::Tuple(parts) | hir::ExprKind::ArrayLit(parts) => {
                for p in parts {
                    self.value(p);
                }
            }
            hir::ExprKind::Call { callee, args } => {
                match callee {
                    hir::Callee::Method { recv, .. } | hir::Callee::BaseMethod { recv, .. } => {
                        self.read(recv)
                    }
                    hir::Callee::Value(v) => self.value(v),
                    _ => {}
                }
                for a in args {
                    self.value(a);
                }
            }
            hir::ExprKind::Len(base) | hir::ExprKind::DataPtr(base) => self.read(base),
            // `construct(dst, …)`, `destroy(dst)` and `move_out(dst)` act on `dst` in place
            // (spec §14.3).
            hir::ExprKind::Builtin { name, args }
                if matches!(name.as_str(), "construct" | "destroy" | "move_out") =>
            {
                let mut rest = args.into_iter();
                if let Some(dst) = rest.next() {
                    self.read(dst);
                }
                for a in rest {
                    self.value(a);
                }
            }
            hir::ExprKind::Builtin { args, .. } | hir::ExprKind::Intrinsic { args, .. } => {
                for a in args {
                    self.value(a);
                }
            }
            hir::ExprKind::Block(b) => self.block(b),
            hir::ExprKind::If { cond, then, else_branch } => {
                self.value(cond);
                self.block(then);
                if let Some(b) = else_branch {
                    self.block(b);
                }
            }
            hir::ExprKind::Guarded { init, value, .. } => {
                self.value(init);
                self.value(value);
            }
            hir::ExprKind::Formatted { args, body, .. } => {
                for a in args {
                    self.value(a);
                }
                self.block(body);
            }
            // A reference binding refers into the operand; a plain one takes the value out,
            // as `or` does (18.61).
            hir::ExprKind::IfLet { binding, operand, then, else_branch } => {
                if matches!(self.body.local(binding).ty, Ty::Ref { .. }) {
                    self.read(operand);
                } else {
                    self.value(operand);
                }
                self.block(then);
                if let Some(b) = else_branch {
                    self.block(b);
                }
            }
            hir::ExprKind::Switch { subject, arms, default } => {
                self.read(subject);
                for a in arms {
                    self.block(a.body);
                }
                if let Some(b) = default {
                    self.block(b);
                }
            }
            hir::ExprKind::Or { operand, clause } => {
                self.value(operand);
                match clause {
                    hir::OrClause::Value(v) => self.value(v),
                    hir::OrClause::Block { body, .. } => self.block(body),
                    hir::OrClause::Ignore | hir::OrClause::Unreachable => {}
                }
            }
            hir::ExprKind::Return(Some(v)) => {
                if matches!(self.body.expr(v).kind, hir::ExprKind::Local(_)) {
                    self.read(v);
                } else {
                    self.value(v);
                }
            }
            hir::ExprKind::SliceRange { base, start, end } => {
                self.read(base);
                for x in [start, end].into_iter().flatten() {
                    self.value(x);
                }
            }
            _ => {}
        }
    }
}
