//! The resolved, typed, desugared tree (spec §17.1).
//!
//! M4 got by without one: C++ has its own inference, so codegen could walk the AST and let
//! the C++ compiler work out what `a + b` meant. M5 cannot. Flow analysis has to say *this
//! local, at this point* is moved-from, which needs stable node identity; and lowering
//! `T?` and fallible returns need a subexpression's type before picking a representation.
//!
//! Three properties are the point of this IR, and each one is a rule for anything added to
//! it later:
//!
//! 1. **Every name is already bound.** There are no paths and no identifiers to look up —
//!    a variable is a [`LocalId`], an item is a [`SymbolId`], a field is an index into the
//!    struct's declaration order. Resolution happens once, in the checker, not again in
//!    every consumer.
//! 2. **Every expression carries its type.** [`Expr::ty`] is what the checker concluded,
//!    after literal defaulting, so consumers never re-infer.
//! 3. **Node identity is stable.** Nodes live in per-body arenas and are named by index, so
//!    a dataflow pass can key a bitset on [`ExprId`] and a diagnostic can point back at a
//!    span without threading references around.
//!
//! Bodies are built by `dz_check` as it checks, rather than by a separate pass, because
//! checking already computes every type this tree stores; a second pass would recompute
//! them and could disagree.

use std::collections::HashMap;

use dz_resolve::SymbolId;
use dz_span::{FileId, Span};

pub mod dump;
pub mod ty;

/// Re-exported so consumers of the HIR do not have to depend on `dz_ast` for the
/// operators its nodes store.
pub use dz_ast::{AssignOp, BinaryOp, UnaryOp};

use ty::Ty;

// ---------------------------------------------------------------------------
// ids
// ---------------------------------------------------------------------------

/// Declares an index newtype over a per-body arena.
macro_rules! id {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
        pub struct $name(pub u32);

        impl $name {
            pub fn index(self) -> usize {
                self.0 as usize
            }
        }
    };
}

id!(ExprId, "An expression within one [`Body`].");
id!(StmtId, "A statement within one [`Body`].");
id!(BlockId, "A block within one [`Body`].");
id!(LocalId, "A local variable, parameter or pattern binding within one [`Body`].");
id!(BodyId, "One function, method or constructor body in the whole program.");

// ---------------------------------------------------------------------------
// program
// ---------------------------------------------------------------------------

/// Every body in the workspace, plus the lookup consumers need to find one.
#[derive(Default)]
pub struct Hir {
    bodies: Vec<Body>,
    index: HashMap<BodyOwner, BodyId>,
}

impl Hir {
    pub fn push(&mut self, body: Body) -> BodyId {
        let id = BodyId(self.bodies.len() as u32);
        self.index.insert(body.owner.clone(), id);
        self.bodies.push(body);
        id
    }

    pub fn body(&self, id: BodyId) -> &Body {
        &self.bodies[id.index()]
    }

    pub fn find(&self, owner: &BodyOwner) -> Option<&Body> {
        self.index.get(owner).map(|id| self.body(*id))
    }

    pub fn bodies(&self) -> impl Iterator<Item = (BodyId, &Body)> {
        self.bodies.iter().enumerate().map(|(i, b)| (BodyId(i as u32), b))
    }

    pub fn len(&self) -> usize {
        self.bodies.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bodies.is_empty()
    }
}

/// What a body belongs to. Free functions and constants have a symbol of their own;
/// members do not, since they are named within a type rather than at module level, so they
/// are keyed by their owner plus their name or position in declaration order.
///
/// `copy` and `drop` get variants of their own rather than being methods named "copy" and
/// "drop", because §6 gives them meaning no ordinary method has: flow analysis has to ask
/// "does this type have a `drop`?" constantly, and that question should not be a string
/// comparison against a method table.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum BodyOwner {
    Fn(SymbolId),
    Method { owner: SymbolId, name: String },
    Ctor { owner: SymbolId, index: usize },
    Copy(SymbolId),
    Drop(SymbolId),
    Const(SymbolId),
    /// The initialiser of a field with a default (spec §7.1).
    FieldDefault { owner: SymbolId, index: usize },
}

/// One function body: its locals and its statement/expression arenas.
pub struct Body {
    pub owner: BodyOwner,
    pub file: FileId,
    /// The declaration's span, for diagnostics that name the function itself.
    pub span: Span,
    /// Parameters, in order. `self` is included when the body has one.
    pub params: Vec<LocalId>,
    /// Default-argument expressions, one slot per parameter (spec §7.2). They live in this
    /// body's arena but are not reachable from `root`, since they are evaluated at the call
    /// site; keeping them here is what makes them reachable at all.
    pub param_defaults: Vec<Option<ExprId>>,
    pub ret: Ty,
    /// `!->` (spec §9.1). The **success** type is in `ret`; this says whether a failure
    /// can also come back. Codegen needs it to pick the C++ return type, and definite
    /// return needs it to know what `or return` propagates.
    pub fallible: bool,
    pub locals: Vec<Local>,
    pub exprs: Vec<Expr>,
    pub stmts: Vec<Stmt>,
    pub blocks: Vec<Block>,
    pub root: BlockId,
}

impl Body {
    pub fn expr(&self, id: ExprId) -> &Expr {
        &self.exprs[id.index()]
    }

    pub fn stmt(&self, id: StmtId) -> &Stmt {
        &self.stmts[id.index()]
    }

    pub fn block(&self, id: BlockId) -> &Block {
        &self.blocks[id.index()]
    }

    pub fn local(&self, id: LocalId) -> &Local {
        &self.locals[id.index()]
    }

    pub fn ty(&self, id: ExprId) -> &Ty {
        &self.exprs[id.index()].ty
    }
}

/// A local variable. Parameters, `let` bindings, `for` and `with` bindings and `case`
/// payload bindings are all locals; flow analysis does not care which is which except when
/// wording a diagnostic.
#[derive(Clone, Debug)]
pub struct Local {
    pub name: String,
    pub ty: Ty,
    pub mutable: bool,
    pub span: Span,
    pub kind: LocalKind,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LocalKind {
    Param,
    Let,
    /// Bound by a pattern: `for x in`, `with (x) { it -> }`, `case Circle(c)`.
    Binding,
}

// ---------------------------------------------------------------------------
// statements
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Block {
    pub span: Span,
    pub stmts: Vec<StmtId>,
}

#[derive(Clone, Debug)]
pub struct Stmt {
    pub span: Span,
    pub kind: StmtKind,
}

#[derive(Clone, Debug)]
pub enum StmtKind {
    /// `let` with an initialiser. A `let` without one stores `None`, which is what
    /// use-before-init analysis keys on; `let _ = f()` has no local but still evaluates.
    Let { local: Option<LocalId>, init: Option<ExprId> },
    /// `let (a, b) = e` — the tuple is evaluated once, then destructured in order.
    LetTuple { locals: Vec<Option<LocalId>>, init: ExprId },
    /// Compound assignment keeps its operator: `a += b` evaluates `a` once, which matters
    /// once places can have side effects.
    Assign { target: ExprId, op: AssignOp, value: ExprId },
    Expr(ExprId),
    While { cond: ExprId, body: BlockId },
    /// `for x in subject`, or `for i, x in subject` — two bindings mean index and
    /// element (spec §10.4). The subject's type decides what iteration means.
    For { bindings: Vec<Option<LocalId>>, subject: ExprId, body: BlockId },
    /// Runs at scope exit in reverse order, after drops (spec §6.5).
    Defer(StmtId),
    /// A declaration nested in a body. It is checked and emitted as an item; the statement
    /// position only records where it appeared.
    Item(SymbolId),
    Error,
}

// ---------------------------------------------------------------------------
// expressions
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Expr {
    pub span: Span,
    /// What the checker concluded, after literal defaulting: never `IntLit`/`FloatLit`.
    pub ty: Ty,
    pub kind: ExprKind,
}

#[derive(Clone, Debug)]
pub enum ExprKind {
    /// The checker already reported why. Consumers must not report again.
    Error,

    // --- literals; text is already parsed, so nobody re-parses it ---
    /// Stored as `i128` so every integer type including `u64` fits (spec §2.5).
    Int(i128),
    Float(f64),
    Bool(bool),
    Char(char),
    /// The decoded bytes, escapes already applied.
    Str(String),
    /// `null` — only ever a `^T` (spec §5.2).
    Null,
    /// `none` — the empty case of `T?` (spec §9.2).
    NoneOpt,

    // --- places ---
    Local(LocalId),
    Const(SymbolId),
    /// A function used as a value.
    FnItem(SymbolId),
    /// Field access resolved to an index into the type's declaration order.
    Field { base: ExprId, index: usize },
    TupleIndex { base: ExprId, index: usize },
    Index { base: ExprId, index: ExprId },
    Deref(ExprId),
    Ref { mutable: bool, place: ExprId },

    // --- operators ---
    Unary { op: UnaryOp, operand: ExprId },
    Binary { op: BinaryOp, lhs: ExprId, rhs: ExprId },
    Cast { operand: ExprId },
    Range { start: Option<ExprId>, end: Option<ExprId>, inclusive: bool },

    // --- aggregates ---
    /// Fields in declaration order, `None` where the declaration's default applies
    /// (spec §7.1). Codegen never has to consult the declaration to know the order.
    StructLit { ty: SymbolId, fields: Vec<Option<ExprId>> },
    Tuple(Vec<ExprId>),
    ArrayLit(Vec<ExprId>),
    /// A bare enum constant.
    EnumVariant { owner: SymbolId, index: usize },
    /// A union variant, with its payload's fields in the variant's declaration order and
    /// `None` where a default applies — the same shape as [`ExprKind::StructLit`], and
    /// empty for a variant that has no payload (spec §7.5).
    UnionVariant { owner: SymbolId, index: usize, payload: Vec<Option<ExprId>> },

    // --- calls ---
    Call { callee: Callee, args: Vec<ExprId> },

    // --- ownership (spec §6) ---
    Move(ExprId),

    // --- control flow used as a value ---
    Block(BlockId),
    If { cond: ExprId, then: BlockId, else_branch: Option<BlockId> },
    Switch { subject: ExprId, arms: Vec<Arm>, default: Option<BlockId> },
    /// `e or clause` — the one failure eliminator (spec §9.4). `operand` is fallible: a
    /// call to a `!->` function, a `T?`, or a `^T`. The node's own type is the success
    /// type, so consumers never see the failure shape.
    Or { operand: ExprId, clause: OrClause },
    Return(Option<ExprId>),
    Break { value: Option<ExprId> },
    Continue,

    /// `println`, `sizeof`, `panic`, … — temporary builtins that become stdlib functions
    /// at M12. Kept as a node of their own so that when they do, only this variant and its
    /// consumers disappear (deferral register, `docs/PROGRESS.md`).
    Builtin { name: String, args: Vec<ExprId> },
    /// `#location`, `#file` (spec §12.7).
    Intrinsic { name: String, args: Vec<ExprId> },
}

/// What a call actually calls, decided by the checker rather than re-derived downstream.
#[derive(Clone, Debug)]
pub enum Callee {
    Fn(SymbolId),
    /// `recv.name(...)`, with the receiver evaluated once.
    Method { recv: ExprId, owner: SymbolId, name: String },
    /// A constructor, by position in the type's declaration order (spec §7.2).
    Ctor { owner: SymbolId, index: usize },
    /// Calling a value of function type.
    Value(ExprId),
}

#[derive(Clone, Debug)]
pub struct Arm {
    pub span: Span,
    /// Variant indices in the subject's type; `case A, B` gives two.
    pub variants: Vec<usize>,
    /// `case Circle(c)` binds the payload by reference.
    pub binding: Option<LocalId>,
    pub body: BlockId,
}

/// The right-hand side of an `or` (spec §9.4). Every form either produces the success
/// type or diverges, and the checker has already verified which.
#[derive(Clone, Debug)]
pub enum OrClause {
    /// `or fallback`, including the diverging expressions `or return`, `or break`,
    /// `or continue` and `or panic(...)`.
    Value(ExprId),
    /// `or { ... }` / `or { e -> ... }`.
    Block { binding: Option<LocalId>, body: BlockId },
    /// `or ignore` — the value is deliberately discarded.
    Ignore,
    /// `or unreachable` — the failure is asserted not to happen. Panics in debug, and in
    /// release the success path may be assumed (spec §9.4).
    Unreachable,
}
