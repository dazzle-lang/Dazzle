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
#[derive(Default, Clone)]
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
    Method {
        owner: SymbolId,
        name: String,
    },
    Ctor {
        owner: SymbolId,
        index: usize,
    },
    Copy(SymbolId),
    Drop(SymbolId),
    Const(SymbolId),
    /// A `static`'s initialiser (spec §7.7).
    Static(SymbolId),
    /// The value of a constant declared in a type (spec §7.3).
    AssocConst {
        owner: SymbolId,
        name: String,
    },
    /// The initialiser of a field with a default (spec §7.1).
    FieldDefault {
        owner: SymbolId,
        index: usize,
    },
    /// The initialiser of a union variant's payload field with a default, as a struct
    /// field's (spec §7.5): field `index` of variant `variant`.
    PayloadDefault {
        owner: SymbolId,
        variant: usize,
        index: usize,
    },
    /// A trait method with no body, kept only for its parameters' default arguments: a
    /// trait object's forwarding members declare them (spec §7.2, §7.6).
    TraitDefaults {
        owner: SymbolId,
        name: String,
    },
    /// A method of an `impl Trait for Type` (spec §7.6). An implementation has no name, so
    /// it is addressed by its position in the checker's table of them.
    ImplMethod {
        imp: u32,
        name: String,
    },
}

/// One function body: its locals and its statement/expression arenas.
///
/// `Clone` because monomorphization makes one concrete copy per instantiation (spec §8.4,
/// [`substitute_body`]).
#[derive(Clone, Debug)]
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
    /// A constructor's `: UiElement(w)`, as a call to the base's constructor — in this
    /// body's arena, so its arguments see the constructor's parameters, but not reachable
    /// from `root`, because C++ runs it before the body (spec §7.3, 18.32).
    pub base_init: Option<ExprId>,
    /// The blocks written `unsafe { … }`, where what the checker cannot see is the
    /// programmer's word: ties allow an untied pointer to become a reference there (spec
    /// §5.2, §6.7). An `unsafe fn`'s whole body is one, which its signature says.
    pub unsafe_blocks: Vec<BlockId>,
    /// The reads written `a.clone()` (spec §6.3, D-0106): each is the place itself, read by
    /// value, which every later phase already treats as a copy, built where placement puts
    /// it. Listed so that nothing turns one into a move: every other by-value read of a
    /// value that is not plain data is a `Move`.
    pub clones: Vec<ExprId>,
    /// The overload a method call of several of one name reached, by its method key
    /// (`count#1`), where the call node carries only the name: an imported C++ method's
    /// casts at the boundary are its own (spec §15.2, T-0017).
    pub method_keys: Vec<(ExprId, String)>,
    /// The pack parameter, `args: ..T` (spec §8.7), whose tuple C++ takes as one parameter
    /// per element and builds at the top of the body, so that the function is called, and
    /// taken as a value, with the elements as its arguments.
    pub pack_param: Option<LocalId>,
}

impl Body {
    /// Is `e` a read written `a.clone()`?
    pub fn is_clone(&self, e: ExprId) -> bool {
        self.clones.contains(&e)
    }

    /// The method key of the overload call `e` reached, when the name has several.
    pub fn method_key(&self, e: ExprId) -> Option<&str> {
        self.method_keys.iter().find(|(x, _)| *x == e).map(|(_, k)| k.as_str())
    }

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

/// How one local is used within its body.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct LocalUse {
    /// Its value is read somewhere: anything but being the target of an assignment.
    pub read: bool,
    /// It is assigned to after its declaration.
    pub assigned: bool,
}

/// Every local's [`LocalUse`], by [`LocalId`].
///
/// One answer for two consumers: flow analysis warns about a binding nothing reads, and
/// codegen marks the same bindings `[[maybe_unused]]` so the C++ compiler does not warn
/// again about the code the user was already warned about (invariant 7).
pub fn local_uses(body: &Body) -> Vec<LocalUse> {
    let mut uses = vec![LocalUse::default(); body.locals.len()];
    let mut targets = std::collections::HashSet::new();
    for s in &body.stmts {
        if let StmtKind::Assign { target, .. } = &s.kind {
            if let ExprKind::Local(l) = body.expr(*target).kind {
                // Assigning to a `&mut T` writes what it refers to, which is a use of the
                // reference rather than a value thrown away (spec §5.2).
                if matches!(body.local(l).ty, Ty::Ref { mutable: true, .. }) {
                    continue;
                }
                uses[l.index()].assigned = true;
                targets.insert(*target);
            }
        }
    }
    for (i, e) in body.exprs.iter().enumerate() {
        if let ExprKind::Local(l) = e.kind {
            if !targets.contains(&ExprId(i as u32)) {
                uses[l.index()].read = true;
            }
        }
    }
    uses
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
    Let {
        local: Option<LocalId>,
        init: Option<ExprId>,
    },
    /// `let (a, b) = e` — the tuple is evaluated once, then destructured in order.
    LetTuple {
        locals: Vec<Option<LocalId>>,
        init: ExprId,
    },
    /// Compound assignment keeps its operator: `a += b` evaluates `a` once, which matters
    /// once places can have side effects.
    Assign {
        target: ExprId,
        op: AssignOp,
        value: ExprId,
        /// A view struct's constructor binding a reference field in its opening statements,
        /// `m_map = map`: the field now refers to the value, where any other assignment to
        /// a reference writes through it (spec §5.2, §6.7). Only ever with `AssignOp::Assign`.
        bind: bool,
    },
    Expr(ExprId),
    While {
        cond: ExprId,
        body: BlockId,
        label: Option<LoopLabel>,
        /// A `loop` no `break` leaves (spec §10.2, D-0102): it ends only by `return`, so the
        /// statement after it is never reached and a function can end with it.
        forever: bool,
    },
    /// `for e in subject` (spec §10.4). The subject's type decides what iteration means,
    /// `shape` how the element reaches the bindings, and each binding's own type whether it
    /// holds a copy (`e`) or a reference (`&e`, `&mut e`).
    For {
        bindings: Vec<Option<LocalId>>,
        subject: ExprId,
        body: BlockId,
        shape: ForShape,
        label: Option<LoopLabel>,
        /// `parallel for` (spec §22.3): the passes run at the same time, split between
        /// threads, and the statement ends when every pass has.
        parallel: bool,
    },
    /// Runs at scope exit in reverse order, after drops (spec §6.5).
    Defer(StmtId),
    /// `parallel { … }` (spec §22.3, D-0118): each of `calls` is a call returning nothing.
    /// Every argument of every call is evaluated first, in order, on this thread; then the
    /// calls run at the same time, and the statement ends when all of them have. A call the
    /// checker refused is an `Error` node, so the list keeps one entry per statement.
    Parallel {
        calls: Vec<ExprId>,
    },
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
    /// A `static`, which is a place: it can be read, and written inside `unsafe` (§7.7).
    Static(SymbolId),
    /// `Allocators::Global` — a constant declared in a type, evaluated where it is used
    /// (spec §7.3).
    AssocConst {
        owner: SymbolId,
        name: String,
    },
    /// A function used as a value; `type_args` instantiate a generic one (spec §8.1).
    FnItem {
        sym: SymbolId,
        type_args: Vec<Ty>,
    },
    /// A method used as a value, its receiver its first parameter (spec §8.1, T-0261):
    /// `Counter::add` is a `fn(&mut Counter, i32)`. `owner_args` instantiate a generic owner,
    /// `Pair<i32>::bump`; `method_args` a generic method, `World::remove<Health>`.
    MethodItem {
        owner: SymbolId,
        owner_args: Vec<Ty>,
        name: String,
        method_args: Vec<Ty>,
    },
    /// Field access resolved to an index into the type's declaration order.
    Field {
        base: ExprId,
        index: usize,
    },
    TupleIndex {
        base: ExprId,
        index: usize,
    },
    Index {
        base: ExprId,
        index: ExprId,
    },
    /// `xs[a..b]`, `xs[a..]`, `xs[..b]` — a slice of part of an array, a slice or, inside
    /// `unsafe`, a pointer (spec §5.3). A missing end on an array or slice is its length.
    SliceRange {
        base: ExprId,
        start: Option<ExprId>,
        end: Option<ExprId>,
    },
    Deref(ExprId),
    Ref {
        mutable: bool,
        place: ExprId,
        /// A reference to a value that is not a place, used by nothing past the statement
        /// it is in: a formatting macro's argument (spec §12.9). The value, and whatever its
        /// evaluation made, lives until that statement ends, as a call's arguments do —
        /// rather than in a local for the rest of the block, as `&5` does.
        temporary: bool,
    },

    // --- operators ---
    Unary {
        op: UnaryOp,
        operand: ExprId,
    },
    Binary {
        op: BinaryOp,
        lhs: ExprId,
        rhs: ExprId,
    },
    Cast {
        operand: ExprId,
        /// Written `as`, rather than a conversion the checker inserted. Every explicit
        /// pointer cast is `unsafe`, and what it makes is unchecked by ties (spec §6.7).
        explicit: bool,
    },
    Range {
        start: Option<ExprId>,
        end: Option<ExprId>,
        inclusive: bool,
    },

    // --- aggregates ---
    /// Fields in declaration order, `None` where the declaration's default applies
    /// (spec §7.1). Codegen never has to consult the declaration to know the order.
    StructLit {
        ty: SymbolId,
        fields: Vec<Option<ExprId>>,
    },
    Tuple(Vec<ExprId>),
    /// `..t` where `t` holds a pack, `(..T)` or `(i32, ..T)`: elements `from` up to `to` of
    /// `base`, spliced into the call's arguments or the tuple literal it stands in (spec
    /// §8.7). Where the run is the pack's, it is the one entry that fills the `..T` of the
    /// list it matches, and its type is the tuple `(..T)`; an element after the pack, whose
    /// place depends on the pack's length, is a run of one counted from the back, typed as
    /// that element. `base` is a place, read once per element. A tuple of known types is
    /// never spread this way: the checker passes its elements, `t.0`, `t.1`, ….
    Spread {
        base: ExprId,
        from: TupleEnd,
        to: TupleEnd,
    },
    /// More elements of the tuple a [`ExprKind::Spread`] earlier in the same list read,
    /// `from` up to `to`: a spread of `(i32, ..T, bool)` fills three entries of its list, and
    /// takes its tuple once, in the first. So this node has no operand of its own, and
    /// whatever walks the tree meets the tuple once (spec §8.7).
    SpreadRest {
        of: ExprId,
        from: TupleEnd,
        to: TupleEnd,
    },
    /// `..pattern`: `pattern` once for each element of `over`, a pack's tuple `(..A)`, with
    /// the local `elem` standing for the element, of the type `A`'s element (spec §8.7). It
    /// fills the one entry of its list that a pack does, and its type is the tuple of what
    /// the pattern makes. Checked once, as the generic body is; when the body is copied
    /// for an instance, [`map_body_types`] makes it an [`ExprKind::Expanded`], one copy of
    /// `pattern` per element, so nothing after monomorphization meets one.
    Expand {
        over: ExprId,
        elem: LocalId,
        pack: ty::Param,
        pattern: ExprId,
    },
    /// The copies an [`ExprKind::Expand`] became, one per element: several values standing
    /// in the one entry of their list, as a spread's elements do.
    Expanded(Vec<ExprId>),
    ArrayLit(Vec<ExprId>),
    /// A bare enum constant.
    EnumVariant {
        owner: SymbolId,
        index: usize,
    },
    /// A union variant, with its payload's fields in the variant's declaration order and
    /// `None` where a default applies — the same shape as [`ExprKind::StructLit`], and
    /// empty for a variant that has no payload (spec §7.5).
    UnionVariant {
        owner: SymbolId,
        index: usize,
        payload: Vec<Option<ExprId>>,
    },

    // --- calls ---
    Call {
        callee: Callee,
        args: Vec<ExprId>,
    },

    // --- ownership (spec §6) ---
    Move(ExprId),

    // --- control flow used as a value ---
    Block(BlockId),
    If {
        cond: ExprId,
        then: BlockId,
        else_branch: Option<BlockId>,
    },
    /// `if let binding = operand` (spec §9.4). `operand` is a `T?` or a `^T`; `binding`
    /// holds what `or` would produce from it — the `T`, or a `&mut T` — in `then` only.
    IfLet {
        binding: LocalId,
        operand: ExprId,
        then: BlockId,
        else_branch: Option<BlockId>,
    },
    Switch {
        subject: ExprId,
        arms: Vec<Arm>,
        default: Option<BlockId>,
    },
    /// `guard = init`, then `value` — what `or` and `if let` on a `Nullable` handle stand
    /// for (spec §14.5, 18.62). `guard` is a local with no name anyone can write, holding
    /// the handle's `Guard` (a strong `Rc` for an `Rc` or a `Weak`) until the end of the
    /// block the expression sits in, so what `value` points at cannot be freed under it.
    /// `value` is the target as a pointer, which the enclosing `or` or `if let` checks.
    Guarded {
        guard: LocalId,
        init: ExprId,
        value: ExprId,
    },
    /// A formatting macro's invocation (spec §12.9, D-0080). `args` are evaluated as a
    /// call's arguments are — left to right, all of them first — and each is bound to the
    /// local beside it in `params`; then `body` runs, and its final expression is the
    /// node's value. What the arguments' evaluation made lives until `body` has ended, as a
    /// call's temporaries live until it returns, which is what lets `body` write each
    /// argument with a direct `Format::format` call per placeholder rather than through a
    /// vtable.
    Formatted {
        args: Vec<ExprId>,
        params: Vec<LocalId>,
        body: BlockId,
    },
    /// `e or clause` — the one failure eliminator (spec §9.4). `operand` is fallible: a
    /// call to a `!->` function, a `T?`, or a `^T`. The node's own type is the success
    /// type, so consumers never see the failure shape.
    Or {
        operand: ExprId,
        clause: OrClause,
    },
    Return(Option<ExprId>),
    /// `break`, or `break outer` naming a loop further out than the innermost, by that
    /// loop's [`LoopLabel::id`] (spec §10.2).
    Break(Option<u32>),
    /// `continue`, likewise.
    Continue(Option<u32>),

    /// `xs.len` on an array or slice (spec §5.3). A node of its own because it is not a
    /// field: nothing is stored under that name, and codegen asks the container itself.
    /// Before this existed `.len` was built as `Error` and emitted as a literal `0`, so
    /// `for i in 0..xs.len` compiled to a loop that never ran.
    Len(ExprId),
    /// `.ptr` on an array or slice: its first element as a `^T`, for unchecked access inside
    /// `unsafe` (spec §5.3). Only a writable view has one, since a `^T` may be written
    /// through.
    DataPtr(ExprId),

    /// `T::is_trivially_copyable`, `sizeof(T)` — a question about a type (spec §5.5).
    ///
    /// Inside a generic body `ty` is a `Ty::Param`, so the answer is not known until
    /// monomorphization supplies the argument; that is where it is answered, and where an
    /// `if` made only of these is folded so the dead branch is never emitted (§8.3). Kept
    /// as a node rather than evaluated in the checker for exactly that reason.
    TypeQuery {
        ty: Ty,
        query: TypeQuery,
    },
    /// `typeinfo_of<T>()` or `typeid_of<T>()` (spec §13.1): answered from the type table
    /// codegen writes, for a type monomorphization has made concrete.
    Reflect {
        ty: Ty,
        info: bool,
    },

    /// `println`, `sizeof`, `panic`, … — temporary builtins that become stdlib functions
    /// at M12. Kept as a node of their own so that when they do, only this variant and its
    /// consumers disappear (deferral register, `docs/tasks/`).
    Builtin {
        name: String,
        args: Vec<ExprId>,
    },
    /// `#location`, `#file` (spec §12.7).
    Intrinsic {
        name: String,
        args: Vec<ExprId>,
    },
    /// An argument the call left out, standing for the callee's own default for parameter
    /// `index` (not counting `self`). Only where a later parameter is passed by name — a
    /// default at the end needs nothing, since C++ fills it in (spec §7.2).
    DefaultArg(usize),
}

/// A concrete copy of a generic body (spec §8.4): every type in it with `map` applied.
///
/// Types are the only thing that differs between instantiations — node ids, locals and
/// control flow are identical — so the copy is structural and exact. Besides each node's
/// and local's own type, only two node kinds carry a type inside them: a type query and a
/// generic call's solved arguments.
pub fn substitute_body(body: &Body, map: &HashMap<u32, Ty>) -> Body {
    substitute_body_with(body, map, None)
}

/// [`substitute_body`], and also what `Self` stands for — which a trait's default method
/// needs when it is copied into a type that inherits it rather than writing it (spec §7.6).
pub fn substitute_body_with(body: &Body, map: &HashMap<u32, Ty>, self_ty: Option<&Ty>) -> Body {
    map_body_types(body, |t| ty::substitute_with(t, map, self_ty))
}

/// A copy of `body` with `f` applied to every type in it: each node's and local's own type,
/// the return type, and the two node kinds that carry a type inside them.
pub fn map_body_types(body: &Body, f: impl Fn(&Ty) -> Ty) -> Body {
    let mut b = body.clone();
    let s = |t: &Ty| f(t);
    b.ret = s(&b.ret);
    for l in &mut b.locals {
        l.ty = s(&l.ty);
    }
    for e in &mut b.exprs {
        map_node_types(e, &s);
    }
    expand_patterns(&mut b);
    b
}

/// A node's own type, and the types inside the two kinds that carry them, with `s` applied.
fn map_node_types(e: &mut Expr, s: &dyn Fn(&Ty) -> Ty) {
    e.ty = s(&e.ty);
    match &mut e.kind {
        ExprKind::TypeQuery { ty: queried, .. } | ExprKind::Reflect { ty: queried, .. } => {
            *queried = s(queried)
        }
        ExprKind::Call { callee: Callee::Fn { type_args, .. }, .. }
        | ExprKind::FnItem { type_args, .. } => {
            for t in type_args.iter_mut() {
                *t = s(t);
            }
        }
        ExprKind::Call { callee: Callee::Assoc { type_args, method_args, .. }, .. }
        | ExprKind::MethodItem { owner_args: type_args, method_args, .. } => {
            for t in type_args.iter_mut().chain(method_args.iter_mut()) {
                *t = s(t);
            }
        }
        ExprKind::Call { callee: Callee::Method { method_args, .. }, .. } => {
            for t in method_args.iter_mut() {
                *t = s(t);
            }
        }
        _ => {}
    }
}

/// Each `..pattern` whose pack is now of known types made into one copy of the pattern per
/// element, the element read from the tuple as `t.0`, `t.1`, … (spec §8.7). A pack still
/// passed on as a pack, in a copy that is not yet an instance, is left as it is.
fn expand_patterns(b: &mut Body) {
    let ids: Vec<ExprId> = (0..b.exprs.len() as u32)
        .map(ExprId)
        .filter(|e| matches!(b.expr(*e).kind, ExprKind::Expand { .. }))
        .collect();
    for id in ids {
        let ExprKind::Expand { over, elem, pack, pattern } = b.expr(id).kind.clone() else {
            continue;
        };
        let place = match b.expr(over).kind {
            ExprKind::Move(p) => p,
            _ => over,
        };
        let parts = match b.ty(place) {
            Ty::Tuple(parts) if parts.iter().all(|p| ty::pack_entry(p).is_none()) => parts.clone(),
            _ => continue,
        };
        let mut copies = Vec::with_capacity(parts.len());
        for (index, part) in parts.iter().enumerate() {
            let mut map = HashMap::new();
            map.insert(pack.element().id, part.clone());
            let read = Element { local: elem, place, index, ty: part.clone() };
            copies.push(copy_tree(b, pattern, &map, &read));
        }
        b.exprs[id.index()].kind = ExprKind::Expanded(copies);
    }
    // A variadic slice's array holds what the copies became: its length is known now.
    for i in 0..b.exprs.len() {
        let ExprKind::ArrayLit(parts) = &b.exprs[i].kind else { continue };
        if !parts.iter().any(|p| matches!(b.expr(*p).kind, ExprKind::Expanded(_))) {
            continue;
        }
        let len: usize = parts
            .iter()
            .map(|p| match &b.expr(*p).kind {
                ExprKind::Expanded(c) => c.len(),
                _ => 1,
            })
            .sum();
        let elem = match &b.exprs[i].ty {
            Ty::Array { elem, .. } | Ty::Slice { elem, .. } => elem.clone(),
            _ => continue,
        };
        // None at all is the empty slice a variadic call with no arguments passes (§7.2).
        if len == 0 {
            b.exprs[i].ty = Ty::Slice { mutable: false, elem };
            b.exprs[i].kind = ExprKind::ArrayLit(Vec::new());
        } else {
            b.exprs[i].ty = Ty::Array { len: len as u64, elem };
        }
    }
}

/// Each of `xs` replaced by `f`'s answer.
fn all(xs: &[ExprId], f: &mut dyn FnMut(ExprId) -> ExprId) -> Vec<ExprId> {
    xs.iter().map(|x| f(*x)).collect()
}

/// What a pattern's element local becomes in one copy: element `index` of `place`.
struct Element {
    local: LocalId,
    place: ExprId,
    index: usize,
    ty: Ty,
}

/// A copy of the tree at `e`, with `map` applied to its types and the element local read
/// from its tuple (spec §8.7). The checker allows in a pattern only what this copies.
fn copy_tree(b: &mut Body, e: ExprId, map: &HashMap<u32, Ty>, read: &Element) -> ExprId {
    let mut node = b.expr(e).clone();
    if let ExprKind::Local(l) = node.kind {
        if l == read.local {
            let base = copy_tree(b, read.place, &HashMap::new(), read);
            node.kind = ExprKind::TupleIndex { base, index: read.index };
            node.ty = read.ty.clone();
            return push_expr(b, node);
        }
    }
    let kind = remap_children(&node.kind, &mut |c| copy_tree(b, c, map, read));
    node.kind = kind.unwrap_or(ExprKind::Error);
    map_node_types(&mut node, &|t| ty::substitute(t, map));
    push_expr(b, node)
}

fn push_expr(b: &mut Body, e: Expr) -> ExprId {
    let id = ExprId(b.exprs.len() as u32);
    b.exprs.push(e);
    id
}

/// `kind` with each node it holds replaced by `f`'s answer, for the kinds a pattern may
/// be made of (spec §8.7). `None` for one that holds a block, a local or a jump, which a
/// pattern may not.
pub fn remap_children(kind: &ExprKind, f: &mut dyn FnMut(ExprId) -> ExprId) -> Option<ExprKind> {
    use ExprKind as K;
    Some(match kind {
        K::Error
        | K::Int(_)
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
        | K::MethodItem { .. }
        | K::EnumVariant { .. }
        | K::TypeQuery { .. }
        | K::Reflect { .. }
        | K::DefaultArg(_) => kind.clone(),
        K::Field { base, index } => K::Field { base: f(*base), index: *index },
        K::TupleIndex { base, index } => K::TupleIndex { base: f(*base), index: *index },
        K::Index { base, index } => {
            let base = f(*base);
            K::Index { base, index: f(*index) }
        }
        K::SliceRange { base, start, end } => {
            let base = f(*base);
            let start = start.map(&mut *f);
            K::SliceRange { base, start, end: end.map(&mut *f) }
        }
        K::Deref(x) => K::Deref(f(*x)),
        K::Ref { mutable, place, temporary } => {
            K::Ref { mutable: *mutable, place: f(*place), temporary: *temporary }
        }
        K::Unary { op, operand } => K::Unary { op: *op, operand: f(*operand) },
        K::Binary { op, lhs, rhs } => {
            let lhs = f(*lhs);
            K::Binary { op: *op, lhs, rhs: f(*rhs) }
        }
        K::Cast { operand, explicit } => K::Cast { operand: f(*operand), explicit: *explicit },
        K::Range { start, end, inclusive } => {
            let start = start.map(&mut *f);
            K::Range { start, end: end.map(&mut *f), inclusive: *inclusive }
        }
        K::StructLit { ty, fields } => {
            K::StructLit { ty: *ty, fields: fields.iter().map(|x| x.map(&mut *f)).collect() }
        }
        K::UnionVariant { owner, index, payload } => K::UnionVariant {
            owner: *owner,
            index: *index,
            payload: payload.iter().map(|x| x.map(&mut *f)).collect(),
        },
        K::Tuple(xs) => K::Tuple(all(xs, f)),
        K::ArrayLit(xs) => K::ArrayLit(all(xs, f)),
        K::Expanded(xs) => K::Expanded(all(xs, f)),
        K::Builtin { name, args } => K::Builtin { name: name.clone(), args: all(args, f) },
        K::Intrinsic { name, args } => K::Intrinsic { name: name.clone(), args: all(args, f) },
        K::Move(x) => K::Move(f(*x)),
        K::Len(x) => K::Len(f(*x)),
        K::DataPtr(x) => K::DataPtr(f(*x)),
        K::Spread { base, from, to } => K::Spread { base: f(*base), from: *from, to: *to },
        K::Call { callee, args } => {
            let callee = match callee {
                Callee::Method { recv, owner, name, method_args } => Callee::Method {
                    recv: f(*recv),
                    owner: *owner,
                    name: name.clone(),
                    method_args: method_args.clone(),
                },
                Callee::BaseMethod { recv, owner, name } => {
                    Callee::BaseMethod { recv: f(*recv), owner: *owner, name: name.clone() }
                }
                Callee::Value(v) => Callee::Value(f(*v)),
                other => other.clone(),
            };
            K::Call { callee, args: all(args, f) }
        }
        K::Block(_)
        | K::If { .. }
        | K::IfLet { .. }
        | K::Switch { .. }
        | K::Guarded { .. }
        | K::Formatted { .. }
        | K::Or { .. }
        | K::Return(_)
        | K::Break(_)
        | K::Continue(_)
        | K::SpreadRest { .. }
        | K::Expand { .. } => return None,
    })
}

/// The compile-time questions every type answers (spec §5.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TypeQuery {
    /// `T::size`, and `sizeof(T)`.
    Size,
    /// `T::align`, and `alignof(T)`.
    Align,
    /// `T::name` — the source-level name.
    Name,
    /// `T::is_trivially_copyable` — no drop, no copy, every field trivially copyable.
    IsTriviallyCopyable,
    /// `T::is_relocatable` — a value may be moved by copying its bytes and forgetting the
    /// source: every type but a type parameter and an imported C++ class that runs code of
    /// its own when moved or destroyed.
    IsRelocatable,
    /// `T::is_copyable` — has an implicit or explicit copy (§6.3).
    IsCopyable,
    /// `T::has_drop`.
    HasDrop,
    /// `countof(A)`: how many types the pack `A` holds, asked of its tuple `(..A)`, which
    /// monomorphization makes one of known length (spec §8.7). There is no `A::count`: a
    /// name after `::` could be an element's own.
    Count,
}

impl TypeQuery {
    pub fn from_name(name: &str) -> Option<TypeQuery> {
        Some(match name {
            "size" => TypeQuery::Size,
            "align" => TypeQuery::Align,
            "name" => TypeQuery::Name,
            "is_trivially_copyable" => TypeQuery::IsTriviallyCopyable,
            "is_relocatable" => TypeQuery::IsRelocatable,
            "is_copyable" => TypeQuery::IsCopyable,
            "has_drop" => TypeQuery::HasDrop,
            _ => return None,
        })
    }

    pub fn name(&self) -> &'static str {
        match self {
            TypeQuery::Size => "size",
            TypeQuery::Align => "align",
            TypeQuery::Name => "name",
            TypeQuery::IsTriviallyCopyable => "is_trivially_copyable",
            TypeQuery::IsRelocatable => "is_relocatable",
            TypeQuery::IsCopyable => "is_copyable",
            TypeQuery::HasDrop => "has_drop",
            TypeQuery::Count => "count",
        }
    }

    /// The type of the answer.
    pub fn result(&self) -> Ty {
        match self {
            TypeQuery::Size | TypeQuery::Align | TypeQuery::Count => Ty::Int(ty::IntTy::Usize),
            TypeQuery::Name => Ty::Str,
            TypeQuery::IsTriviallyCopyable
            | TypeQuery::IsRelocatable
            | TypeQuery::IsCopyable
            | TypeQuery::HasDrop => Ty::Bool,
        }
    }
}

/// A loop's label, when a `break` or `continue` inside a nested loop names it. The innermost
/// loop needs no label, and C++ warns about one nothing uses (spec §10.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoopLabel {
    /// Unique within the body, so two sibling loops may share a Dazzle label.
    pub id: u32,
    pub name: String,
    /// Some `break` names it from inside a nested loop.
    pub breaks: bool,
    /// Some `continue` names it from inside a nested loop.
    pub continues: bool,
}

/// How a `for` loop's element reaches its bindings (spec §10.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ForShape {
    /// `for e in xs` — one binding, the whole element.
    Single,
    /// `for entity, btn, state in query` — the element is a tuple, one binding per part.
    Tuple,
    /// `for i, e in xs.indexed()` — `subject` is `xs` itself; the first binding counts
    /// passes from zero and the second is the element.
    Indexed,
    /// `for e in move(xs)` — `subject` is `xs`, which the loop consumes, moving each
    /// element into `e`.
    Consuming,
}

/// One end of a [`ExprKind::Spread`]'s run of elements, counted from the tuple's front or
/// from its back: before monomorphization a pack's length is not known, so an element
/// after it has no index from the front (spec §8.7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TupleEnd {
    Front(usize),
    Back(usize),
}

impl TupleEnd {
    /// The index this end is in a tuple of `len` elements.
    pub fn index(self, len: usize) -> usize {
        match self {
            TupleEnd::Front(i) => i,
            TupleEnd::Back(i) => len.saturating_sub(i),
        }
    }
}

/// What a call actually calls, decided by the checker rather than re-derived downstream.
#[derive(Clone, Debug)]
pub enum Callee {
    /// A free function. `type_args` is empty unless the function is generic, in which case
    /// it is what the call solved its parameters to (spec §8.1) — inside another generic
    /// body those may themselves be `Ty::Param`, which monomorphization substitutes.
    Fn { sym: SymbolId, type_args: Vec<Ty> },
    /// `recv.name(...)`, with the receiver evaluated once. `method_args` are what the call
    /// solved the method's *own* type parameters to — `fn map<U>(&self, …)` — and are empty
    /// for a method with none; the type's parameters come from the receiver (spec §8.1).
    Method { recv: ExprId, owner: SymbolId, name: String, method_args: Vec<Ty> },
    /// `UiElement::draw(self)` — a base's version of a method, named through the base so
    /// that it is called *without* virtual dispatch. This is how an `override` extends
    /// what it replaces instead of discarding it (spec §7.3, 18.32). `owner` is the base
    /// the call is qualified by, and C++ writes it as `recv.UiElement::draw()`.
    BaseMethod { recv: ExprId, owner: SymbolId, name: String },
    /// `Quat::identity()`, `Buffer<f32>::with_capacity(8)` — a function declared in a type
    /// with no receiver (spec §7.3). `type_args` are the type's, empty unless it is generic;
    /// `method_args` the function's own, as for `Method`.
    Assoc { owner: SymbolId, type_args: Vec<Ty>, name: String, method_args: Vec<Ty> },
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
    /// `case 1, MAX` on an integer, `case "idle"` on a `str` (spec §10.3): the constants
    /// the subject is compared against. Empty when the subject is an enum or union.
    pub values: Vec<ExprId>,
    /// `case Circle(c)` binds the payload by reference.
    pub binding: Option<LocalId>,
    pub body: BlockId,
}

/// The right-hand side of an `or` (spec §9.4). Every form either produces the success
/// type or diverges, and the checker has already verified which.
#[derive(Clone, Debug)]
pub enum OrClause {
    /// `or fallback`, including the diverging expressions `or return`, `or break`,
    /// `or continue` and `or panic!(...)`.
    Value(ExprId),
    /// `or { ... }` / `or { e -> ... }`.
    Block { binding: Option<LocalId>, body: BlockId },
    /// `or ignore` — the value is deliberately discarded.
    Ignore,
    /// `or unreachable` — the failure is asserted not to happen. Panics in debug, and in
    /// release the success path may be assumed (spec §9.4).
    Unreachable,
}

#[cfg(test)]
mod tests {
    use super::TypeQuery;

    /// Resolution accepts exactly the queries this type can represent; the two lists must
    /// not drift, or `T::size` would resolve and then fail to check (spec §5.5).
    #[test]
    fn resolution_and_the_hir_agree_on_the_type_queries() {
        for q in dz_resolve::TYPE_QUERIES {
            let parsed = TypeQuery::from_name(q).expect("a query resolution accepts");
            assert_eq!(parsed.name(), *q);
        }
    }
}
