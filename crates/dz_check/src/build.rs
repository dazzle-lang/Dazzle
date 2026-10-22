//! Building the HIR while checking (spec §17.1, `dz_hir`).
//!
//! The checker already computes every type the HIR stores, so it builds the tree as it
//! goes rather than handing the job to a second pass that would have to recompute them and
//! could disagree. This module is only the arenas and the push helpers; the decisions live
//! in `check.rs` next to the rule each one enforces.

use dz_hir::ty::Ty;
use dz_hir::{self as hir, BlockId, BodyOwner, ExprId, LocalId, LocalKind, StmtId};
use dz_span::{FileId, Span};

/// An expression's type together with the node the checker built for it.
///
/// Checking and building are one traversal, so every rule returns both: the type for the
/// rule that consumes it, the id for the parent node being built.
#[derive(Clone, Debug)]
pub(crate) struct Checked {
    pub ty: Ty,
    pub id: ExprId,
}

/// A block's type together with the node the checker built for it.
#[derive(Clone, Debug)]
pub(crate) struct CheckedBlock {
    pub ty: Ty,
    pub id: BlockId,
}

/// A saved position in a [`BodyBuilder`]'s arenas.
#[derive(Clone, Copy)]
pub(crate) struct Mark {
    exprs: usize,
    stmts: usize,
    blocks: usize,
    locals: usize,
}

/// The arenas of one body under construction.
pub(crate) struct BodyBuilder {
    owner: BodyOwner,
    file: FileId,
    span: Span,
    params: Vec<LocalId>,
    param_defaults: Vec<Option<ExprId>>,
    ret: Ty,
    locals: Vec<hir::Local>,
    exprs: Vec<hir::Expr>,
    stmts: Vec<hir::Stmt>,
    blocks: Vec<hir::Block>,
    base_init: Option<ExprId>,
}

impl BodyBuilder {
    pub(crate) fn new(owner: BodyOwner, file: FileId, span: Span, ret: Ty) -> BodyBuilder {
        BodyBuilder {
            owner,
            file,
            span,
            params: Vec::new(),
            param_defaults: Vec::new(),
            ret,
            locals: Vec::new(),
            exprs: Vec::new(),
            stmts: Vec::new(),
            blocks: Vec::new(),
            base_init: None,
        }
    }

    /// Record a constructor's `: Base(…)` (spec §7.3, 18.32). It lives in this body's
    /// arena so its arguments can name the constructor's parameters, but outside `root`,
    /// because C++ runs it before the body.
    pub(crate) fn set_base_init(&mut self, e: ExprId) {
        self.base_init = Some(e);
    }

    /// Is this body a constructor's? Only a constructor builds its base.
    pub(crate) fn owner_is_ctor(&self) -> bool {
        matches!(self.owner, BodyOwner::Ctor { .. })
    }

    /// Push a node. The type is stored exactly as the checker concluded it, *including*
    /// an untyped literal type: `0` in `0..xs.len` is still `{integer}` at this point, and
    /// defaulting it here would decide it is `i32` before the range ever sees `usize`.
    /// [`BodyBuilder::finish`] settles whatever is still untyped once checking is done.
    pub(crate) fn expr(&mut self, span: Span, ty: Ty, kind: hir::ExprKind) -> Checked {
        let id = ExprId(self.exprs.len() as u32);
        self.exprs.push(hir::Expr { span, ty: ty.clone(), kind });
        Checked { ty, id }
    }

    /// Give a node the type that was only known after it was built: a `none` arm, typed by
    /// the arms it was joined with (spec §9.1, 18.53).
    pub(crate) fn set_expr_type(&mut self, id: ExprId, ty: Ty) {
        self.exprs[id.index()].ty = ty;
    }

    /// How a local was introduced.
    pub(crate) fn local_kind(&self, id: LocalId) -> LocalKind {
        self.locals[id.index()].kind
    }

    /// A local's type, as it was declared.
    pub(crate) fn local_ty(&self, id: LocalId) -> &Ty {
        &self.locals[id.index()].ty
    }

    /// What a node already built turned out to be.
    pub(crate) fn kind(&self, id: ExprId) -> &hir::ExprKind {
        &self.exprs[id.index()].kind
    }

    /// The value of a block that is nothing but one expression — `unsafe { &mut X }`.
    pub(crate) fn block_value(&self, id: BlockId) -> Option<ExprId> {
        match self.blocks[id.index()].stmts.as_slice() {
            [only] => match self.stmts[only.index()].kind {
                hir::StmtKind::Expr(e) => Some(e),
                _ => None,
            },
            _ => None,
        }
    }

    pub(crate) fn stmt(&mut self, span: Span, kind: hir::StmtKind) -> StmtId {
        let id = StmtId(self.stmts.len() as u32);
        self.stmts.push(hir::Stmt { span, kind });
        id
    }

    pub(crate) fn block(&mut self, span: Span, stmts: Vec<StmtId>) -> BlockId {
        let id = BlockId(self.blocks.len() as u32);
        self.blocks.push(hir::Block { span, stmts });
        id
    }

    /// Fill in a block reserved earlier. Needed where a block has to exist before its
    /// contents do, e.g. an `else` branch built from an already-checked expression.
    pub(crate) fn set_block_stmts(&mut self, id: BlockId, stmts: Vec<StmtId>) {
        self.blocks[id.index()].stmts = stmts;
    }

    pub(crate) fn local(
        &mut self,
        name: &str,
        ty: Ty,
        mutable: bool,
        span: Span,
        kind: LocalKind,
    ) -> LocalId {
        let id = LocalId(self.locals.len() as u32);
        self.locals.push(hir::Local { name: name.to_owned(), ty, mutable, span, kind });
        if kind == LocalKind::Param {
            self.params.push(id);
        }
        id
    }

    /// Record a parameter's default-argument expression, in parameter order.
    pub(crate) fn param_default(&mut self, default: Option<ExprId>) {
        self.param_defaults.push(default);
    }

    /// A point the arenas can be wound back to. See [`BodyBuilder::rollback`].
    pub(crate) fn mark(&self) -> Mark {
        Mark {
            exprs: self.exprs.len(),
            stmts: self.stmts.len(),
            blocks: self.blocks.len(),
            locals: self.locals.len(),
        }
    }

    /// Discard everything built since `mark`.
    ///
    /// Place analysis re-checks an expression that has already been checked, to work out
    /// whether it is assignable, and throws the answer away. Without this the arena would
    /// collect a duplicate subtree per query — nodes nothing points at, which every later
    /// pass would then have to know to ignore. Sound only because the ids being dropped
    /// were never handed out.
    pub(crate) fn rollback(&mut self, mark: Mark) {
        self.exprs.truncate(mark.exprs);
        self.stmts.truncate(mark.stmts);
        self.blocks.truncate(mark.blocks);
        self.locals.truncate(mark.locals);
    }

    /// Finish the body. `root` is the block the function's statements went into, and
    /// `fallible` is the signature's `!->` (spec §9.1), which the return type alone
    /// cannot say since it holds only the success case.
    pub(crate) fn finish(mut self, root: BlockId, fallible: bool) -> hir::Body {
        // A node still holding `{integer}`/`{float}` here never met a typing context, so
        // its default is exactly what the language says it is (spec §2.5). Settling them
        // now is what lets the HIR promise that no consumer ever sees an untyped literal.
        for e in &mut self.exprs {
            if e.ty.is_untyped_literal() {
                e.ty = e.ty.defaulted();
            }
        }
        for l in &mut self.locals {
            if l.ty.is_untyped_literal() {
                l.ty = l.ty.defaulted();
            }
        }
        hir::Body {
            owner: self.owner,
            file: self.file,
            span: self.span,
            params: self.params,
            param_defaults: self.param_defaults,
            ret: self.ret,
            fallible,
            locals: self.locals,
            exprs: self.exprs,
            stmts: self.stmts,
            blocks: self.blocks,
            root,
            base_init: self.base_init,
        }
    }
}
