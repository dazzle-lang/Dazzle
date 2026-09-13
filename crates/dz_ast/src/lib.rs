//! The untyped syntax tree (spec §20 is the grammar this mirrors).
//!
//! Every node carries a [`Span`], and every list may contain an `Error` node — the parser
//! recovers rather than bailing (spec §17.2), so a tree always exists even for source that
//! is halfway through being typed.

pub mod dump;

use dz_span::Span;

// ---------------------------------------------------------------------------
// names and paths
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ident {
    pub span: Span,
    pub name: String,
}

impl Ident {
    pub fn new(span: Span, name: impl Into<String>) -> Ident {
        Ident { span, name: name.into() }
    }
}

/// `math::pow`, `Buffer<f32>::with_capacity`. `::` is the only scope separator (spec §3.3).
#[derive(Clone, Debug)]
pub struct Path {
    pub span: Span,
    pub segments: Vec<PathSegment>,
}

#[derive(Clone, Debug)]
pub struct PathSegment {
    pub span: Span,
    pub name: Ident,
    /// `Buffer<f32>` — written without a turbofish (spec §3.3).
    pub generics: Vec<Type>,
}

// ---------------------------------------------------------------------------
// file, imports, items
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default)]
pub struct SourceFileAst {
    pub imports: Vec<Import>,
    pub items: Vec<Item>,
}

#[derive(Clone, Debug)]
pub struct Import {
    pub span: Span,
    pub kind: ImportKind,
}

#[derive(Clone, Debug)]
pub enum ImportKind {
    /// `import engine::render as gfx`
    Module {
        path: Path,
        alias: Option<Ident>,
    },
    /// `import math::{ pow, sqrt }`
    Selective {
        path: Path,
        names: Vec<Ident>,
    },
    /// `import cpp "raylib.h" as rl` (spec §15.1)
    /// `name` is the text between the quotes, which is what the header is looked up by.
    Cpp {
        header: Span,
        name: String,
        alias: Option<Ident>,
    },
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Vis {
    #[default]
    Private,
    /// `protected` — the type's own methods and those of the types that inherit from it,
    /// and nothing else. The fourth visibility, and the only one that exists because of
    /// inheritance (spec §4, §7.3, 18.32).
    Protected,
    /// `pub(module)`
    Module,
    /// `pub`
    Public,
}

#[derive(Clone, Debug)]
pub struct Attr {
    pub span: Span,
    pub name: Ident,
    pub args: Vec<AttrArg>,
}

#[derive(Clone, Debug)]
pub enum AttrArg {
    Ident(Ident),
    Literal(Expr),
    /// `cfg(os = "windows")`
    KeyValue {
        key: Ident,
        value: Expr,
    },
    Nested(Attr),
}

/// Doc comments attached to a declaration, in source order.
pub type Docs = Vec<Span>;

#[derive(Clone, Debug)]
pub struct Item {
    pub span: Span,
    pub docs: Docs,
    pub attrs: Vec<Attr>,
    pub vis: Vis,
    pub kind: ItemKind,
}

#[derive(Clone, Debug)]
pub enum ItemKind {
    Fn(FnDecl),
    Struct(StructDecl),
    Union(UnionDecl),
    Enum(EnumDecl),
    Trait(TraitDecl),
    /// `impl Trait for Type { … }` — the only way a trait is implemented (spec §7.6).
    Impl(ImplDecl),
    Const(ConstDecl),
    /// Parsed shallowly at M1; the body is kept as raw source for M10 (spec §12).
    Macro(MacroDecl),
    /// `#machine Foo(Bar) { ... }` — likewise deferred to M10.
    MacroInvocation {
        name: Ident,
        body: Span,
    },
    Error,
}

#[derive(Clone, Debug)]
pub struct Generics {
    pub span: Span,
    pub params: Vec<GenericParam>,
}

impl Generics {
    pub fn is_empty(&self) -> bool {
        self.params.is_empty()
    }
}

#[derive(Clone, Debug)]
pub struct GenericParam {
    pub span: Span,
    pub name: Ident,
    /// `T: Hash + Eq`
    pub bounds: Vec<Path>,
    /// `Rhs = Self` — used when an instantiation leaves this argument out (spec §11.2).
    /// Only a trailing run of parameters may have defaults, since arguments are positional.
    pub default: Option<Type>,
}

#[derive(Clone, Debug)]
pub struct FnDecl {
    pub span: Span,
    /// `unsafe fn` — its body may touch raw memory, and so a call to it must itself be
    /// inside `unsafe` (spec §5.2).
    pub is_unsafe: bool,
    /// `virtual fn` / `override fn` — which one, if either (spec §7.3, 18.32). A
    /// `virtual fn` with no body makes its struct abstract and not constructible.
    pub dispatch: Dispatch,
    pub name: Ident,
    pub generics: Generics,
    pub self_param: Option<SelfParam>,
    pub params: Vec<Param>,
    pub ret: Option<Type>,
    /// `!->` in place of `->`: this function may fail (spec §9). Fallibility lives on the
    /// signature and never on a type, so there is no `Fallible` in [`TypeKind`].
    pub fallible: bool,
    /// `None` for a trait method with no default body.
    pub body: Option<Block>,
    /// `Button(w: i32) : UiElement(w) { … }` — a constructor's base initialiser, and
    /// `None` on everything else. A base that has a constructor of its own must be
    /// constructed through one (spec §7.3, 18.32).
    pub base_init: Option<BaseInit>,
}

/// How a method is dispatched (spec §7.3, 18.32).
///
/// Static by default: a method call goes to the type the receiver is written as. `virtual`
/// opts one method into runtime dispatch, and `override` replaces one — both spelled, and
/// both checked, so neither can happen by accident.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub enum Dispatch {
    #[default]
    Static,
    /// `virtual fn draw(&self)`. With no body, the struct is abstract.
    Virtual,
    /// `override fn draw(&self)`, replacing a base's `virtual`.
    Override,
}

/// The `: UiElement(w)` of a constructor that builds its base explicitly (spec §7.3).
#[derive(Clone, Debug)]
pub struct BaseInit {
    pub span: Span,
    pub path: Path,
    pub args: Vec<Expr>,
}

#[derive(Clone, Copy, Debug)]
pub struct SelfParam {
    pub span: Span,
    pub is_mut: bool,
}

#[derive(Clone, Debug)]
pub struct Param {
    pub span: Span,
    pub name: Ident,
    pub ty: Type,
    /// Trailing default argument, evaluated at the call site (spec §7.2).
    pub default: Option<Expr>,
}

#[derive(Clone, Debug)]
pub struct StructDecl {
    pub span: Span,
    pub name: Ident,
    pub generics: Generics,
    /// `struct Button : UiElement` — the one base a struct may inherit from (spec §7.3).
    /// Traits are never listed here; they are implemented with `impl` (spec §7.6).
    pub base: Option<Path>,
    pub members: Vec<Member>,
}

/// `impl<T: Copy> Copy for Buffer<T> { … }` (spec §7.6).
#[derive(Clone, Debug)]
pub struct ImplDecl {
    pub span: Span,
    pub generics: Generics,
    /// `None` only when the parser could not read one, having already reported it.
    pub trait_path: Option<Path>,
    pub self_ty: Type,
    /// Methods and associated-type choices; anything else is a checker error.
    pub items: Vec<Member>,
}

#[derive(Clone, Debug)]
pub struct Member {
    pub span: Span,
    pub docs: Docs,
    pub attrs: Vec<Attr>,
    pub vis: Vis,
    pub kind: MemberKind,
}

#[derive(Clone, Debug)]
pub enum MemberKind {
    Field(Field),
    Fn(FnDecl),
    /// `Buffer(cap: u32) { ... }` — name matches the struct.
    Ctor(FnDecl),
    /// An associated type (spec §8.5). `type Item` or `type Item : Format` on a trait,
    /// which names a type without choosing it; `type Item = &T` on an implementer, which
    /// chooses one.
    TypeAlias {
        name: Ident,
        bounds: Vec<Path>,
        ty: Option<Type>,
    },
    Error,
}

#[derive(Clone, Debug)]
pub struct Field {
    pub span: Span,
    pub name: Ident,
    /// `own ^T` — this struct releases the resource (spec §6.3).
    pub is_own: bool,
    pub ty: Type,
    pub default: Option<Expr>,
}

#[derive(Clone, Debug)]
pub struct UnionDecl {
    pub span: Span,
    pub name: Ident,
    pub generics: Generics,
    pub variants: Vec<Variant>,
}

#[derive(Clone, Debug)]
pub struct Variant {
    pub span: Span,
    pub docs: Docs,
    pub name: Ident,
    /// `Circle { radius: f32 }`; empty and `has_payload == false` for a bare variant.
    pub fields: Vec<Field>,
    pub has_payload: bool,
}

#[derive(Clone, Debug)]
pub struct EnumDecl {
    pub span: Span,
    pub name: Ident,
    /// `enum Sprite : u16`
    pub backing: Option<Type>,
    pub variants: Vec<EnumVariant>,
}

#[derive(Clone, Debug)]
pub struct EnumVariant {
    pub span: Span,
    pub docs: Docs,
    pub name: Ident,
    pub value: Option<Expr>,
}

#[derive(Clone, Debug)]
pub struct TraitDecl {
    pub span: Span,
    pub name: Ident,
    pub generics: Generics,
    /// `trait Ord : Eq`
    pub supertraits: Vec<Path>,
    pub items: Vec<Member>,
}

#[derive(Clone, Debug)]
pub struct ConstDecl {
    pub span: Span,
    pub name: Ident,
    pub ty: Option<Type>,
    pub value: Expr,
}

#[derive(Clone, Debug)]
pub struct MacroDecl {
    pub span: Span,
    pub name: Ident,
    /// `declaration`, `expression`, `statement` or `type`.
    pub kind: Ident,
    /// Raw source of the `{ ... }` body, kept verbatim until M10.
    pub body: Span,
}

// ---------------------------------------------------------------------------
// types
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Type {
    pub span: Span,
    pub kind: TypeKind,
}

#[derive(Clone, Debug)]
pub enum TypeKind {
    Path(Path),
    /// `^T` / `^mut T` (spec §5.2)
    Ptr {
        mutable: bool,
        inner: Box<Type>,
    },
    /// `own ^T` / `own ^mut T`
    OwnPtr {
        mutable: bool,
        inner: Box<Type>,
    },
    /// `&T` / `&mut T`
    Ref {
        mutable: bool,
        inner: Box<Type>,
    },
    /// `T?` — postfix, binding to the type name (spec §5.6).
    Optional(Box<Type>),
    /// `[N]T` — a fixed-size value type (spec §5.3).
    Array {
        size: Box<Expr>,
        elem: Box<Type>,
    },
    /// `[]T`, or `[]mut T` — a two-word view whose writability is part of the type
    /// (spec §5.3, §18.22). A fixed array has no such flag: it is a value, so its
    /// mutability comes from the binding that holds it.
    Slice {
        mutable: bool,
        elem: Box<Type>,
    },
    Tuple(Vec<Type>),
    Fn {
        params: Vec<Type>,
        ret: Option<Box<Type>>,
    },
    SelfType,
    Error,
}

// ---------------------------------------------------------------------------
// statements
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Block {
    pub span: Span,
    pub stmts: Vec<Stmt>,
}

#[derive(Clone, Debug)]
pub struct Stmt {
    pub span: Span,
    pub docs: Docs,
    pub kind: StmtKind,
}

#[derive(Clone, Debug)]
pub enum StmtKind {
    Let(LetStmt),
    Assign {
        target: Expr,
        op: AssignOp,
        value: Expr,
    },
    Expr(Expr),
    While(WhileStmt),
    For(ForStmt),
    Defer(Box<Stmt>),
    /// A declaration nested inside a body.
    Item(Box<Item>),
    Error,
}

#[derive(Clone, Debug)]
pub struct LetStmt {
    pub span: Span,
    pub mutable: bool,
    pub pat: Pat,
    pub ty: Option<Type>,
    pub init: Option<Expr>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssignOp {
    Assign,
    Add,
    Sub,
    Mul,
    Div,
    Rem,
}

impl AssignOp {
    pub fn as_str(&self) -> &'static str {
        match self {
            AssignOp::Assign => "=",
            AssignOp::Add => "+=",
            AssignOp::Sub => "-=",
            AssignOp::Mul => "*=",
            AssignOp::Div => "/=",
            AssignOp::Rem => "%=",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Pat {
    pub span: Span,
    pub kind: PatKind,
}

#[derive(Clone, Debug)]
pub enum PatKind {
    Ident(Ident),
    /// `_`
    Wild,
    /// `let (x, y) = point()`
    Tuple(Vec<Pat>),
    Error,
}

#[derive(Clone, Debug)]
pub struct WhileStmt {
    pub span: Span,
    pub label: Option<Ident>,
    pub cond: Expr,
    pub body: Block,
}

#[derive(Clone, Debug)]
pub struct ForStmt {
    pub span: Span,
    pub label: Option<Ident>,
    /// `for entity, btn, state in query` binds one name per part of a tuple element.
    pub bindings: Vec<ForBinding>,
    pub iter: Expr,
    pub body: Block,
}

/// How a `for` or `case` binding holds what it names (spec §10.3, §10.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BindMode {
    /// `e` — a value that cannot be changed. For a `case`, a read-only reference.
    Value,
    /// `&e` — a read-only reference.
    Ref,
    /// `&mut e` — a writable reference.
    RefMut,
}

impl BindMode {
    pub fn prefix(self) -> &'static str {
        match self {
            BindMode::Value => "",
            BindMode::Ref => "&",
            BindMode::RefMut => "&mut ",
        }
    }
}

#[derive(Clone, Debug)]
pub struct ForBinding {
    pub span: Span,
    pub mode: BindMode,
    pub pat: Pat,
}

// ---------------------------------------------------------------------------
// expressions
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Expr {
    pub span: Span,
    pub kind: ExprKind,
}

#[derive(Clone, Debug)]
pub enum ExprKind {
    /// Literal text is kept verbatim; interpretation happens in the type checker.
    Int(String),
    Float(String),
    Str(String),
    RawStr(String),
    Char(String),
    Bool(bool),
    Null,
    None,
    Path(Path),
    /// `.Transition{}` — variant inferred from the expected type (spec §7.3, §7.4).
    DotInfer {
        name: Ident,
        init: Option<Vec<FieldInit>>,
    },
    /// `[1, 2, 3]` — an array of exactly this many elements (spec §5.3).
    ArrayLit(Vec<Expr>),
    StructLit {
        path: Path,
        fields: Vec<FieldInit>,
    },
    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
    },
    Index {
        base: Box<Expr>,
        index: Box<Expr>,
    },
    Field {
        base: Box<Expr>,
        name: Ident,
    },
    Unary {
        op: UnaryOp,
        operand: Box<Expr>,
    },
    Binary {
        op: BinaryOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    Cast {
        expr: Box<Expr>,
        ty: Type,
    },
    Range {
        start: Option<Box<Expr>>,
        end: Option<Box<Expr>>,
        inclusive: bool,
    },
    /// `&e` / `&mut e`
    Ref {
        mutable: bool,
        expr: Box<Expr>,
    },
    /// `p^`
    Deref(Box<Expr>),
    Move(Box<Expr>),
    /// `e or clause` — the one failure eliminator (spec §9.4).
    Or {
        expr: Box<Expr>,
        clause: Box<OrClause>,
    },
    Block(Block),
    /// `unsafe { … }` — a block in which raw-memory operations are allowed (spec §5.2).
    Unsafe(Block),
    If(IfExpr),
    Switch(SwitchExpr),
    /// Diverging expressions, so `f() or return` parses uniformly.
    Return(Option<Box<Expr>>),
    Break {
        label: Option<Ident>,
    },
    Continue {
        label: Option<Ident>,
    },
    /// `#location`, `#file` (spec §12.7)
    Intrinsic {
        name: Ident,
        args: Vec<Expr>,
    },
    /// `vec!(...)` — body kept raw until M10.
    MacroCall {
        name: Ident,
        body: Span,
    },
    Tuple(Vec<Expr>),
    Error,
}

/// The right-hand side of an `or` (spec §9.4). The single rule governing all of these is
/// that the clause must either produce the success type or diverge.
#[derive(Clone, Debug)]
pub enum OrClause {
    /// `or Textures::Missing` — the fallback value. `or return`, `or break`, `or continue`
    /// and `or panic(...)` all arrive here too, as the diverging expressions they already
    /// are, so no clause form needs its own variant.
    Value(Expr),
    /// `or { ... }` and `or { e -> ... }`.
    Block { binding: Option<Ident>, body: Block },
    /// `or ignore` — statement position only; discards the value.
    Ignore(Span),
    /// `or unreachable` — asserts the failure cannot happen.
    Unreachable(Span),
}

#[derive(Clone, Debug)]
pub struct FieldInit {
    pub span: Span,
    /// `None` for positional initialisation, e.g. `Vec3{1, 2, 3}`.
    pub name: Option<Ident>,
    pub value: Expr,
}

#[derive(Clone, Debug)]
pub struct IfExpr {
    pub span: Span,
    /// `if let name = value` (spec §9.4): `cond` is then the value — a `T?` or a `^T` —
    /// and `name` holds what is inside it, for the `then` block only.
    pub binding: Option<IfBinding>,
    pub cond: Box<Expr>,
    pub then: Block,
    /// Either a `Block` expression or another `If`.
    pub else_branch: Option<Box<Expr>>,
}

#[derive(Clone, Debug)]
pub struct IfBinding {
    pub name: Ident,
    pub mutable: bool,
}

#[derive(Clone, Debug)]
pub struct SwitchExpr {
    pub span: Span,
    pub subject: Box<Expr>,
    pub arms: Vec<SwitchArm>,
    pub default: Option<Block>,
}

#[derive(Clone, Debug)]
pub struct SwitchArm {
    pub span: Span,
    /// `case A, B { ... }`
    pub patterns: Vec<CasePat>,
    pub body: Block,
}

/// One label of a `case` (spec §10.3).
///
/// The parser cannot tell `case Idle` on an enum from `case MAX` on an integer — both are a
/// bare name — so anything shaped `Name` or `Name(x)` is a [`CasePat::Variant`], and the
/// checker reads it as a constant when the subject is not an enum or union. Everything else
/// (`case 1`, `case "idle"`, `case -1`, `case lib::MAX`) is a [`CasePat::Value`].
#[derive(Clone, Debug)]
pub enum CasePat {
    Variant(VariantPat),
    Value(Expr),
}

impl CasePat {
    pub fn span(&self) -> Span {
        match self {
            CasePat::Variant(v) => v.span,
            CasePat::Value(e) => e.span,
        }
    }
}

#[derive(Clone, Debug)]
pub struct VariantPat {
    pub span: Span,
    pub name: Ident,
    /// `case Circle(c)` binds the payload by reference.
    pub binding: Option<Ident>,
    /// `case Circle(&mut c)` makes that reference writable (spec §10.3).
    pub mode: BindMode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
    BitNot,
}

impl UnaryOp {
    pub fn as_str(&self) -> &'static str {
        match self {
            UnaryOp::Neg => "-",
            UnaryOp::Not => "!",
            UnaryOp::BitNot => "~",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Shl,
    Shr,
    BitAnd,
    BitXor,
    BitOr,
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    And,
    Or,
}

impl BinaryOp {
    pub fn as_str(&self) -> &'static str {
        match self {
            BinaryOp::Add => "+",
            BinaryOp::Sub => "-",
            BinaryOp::Mul => "*",
            BinaryOp::Div => "/",
            BinaryOp::Rem => "%",
            BinaryOp::Shl => "<<",
            BinaryOp::Shr => ">>",
            BinaryOp::BitAnd => "&",
            // `^` is the pointer sigil, so xor is doubled (spec §2.6).
            BinaryOp::BitXor => "^^",
            BinaryOp::BitOr => "|",
            BinaryOp::Eq => "==",
            BinaryOp::Ne => "!=",
            BinaryOp::Lt => "<",
            BinaryOp::Gt => ">",
            BinaryOp::Le => "<=",
            BinaryOp::Ge => ">=",
            BinaryOp::And => "&&",
            BinaryOp::Or => "||",
        }
    }

    /// Comparison is non-associative (spec §11.1), which is also what removes the need
    /// for a turbofish.
    pub fn is_comparison(&self) -> bool {
        use BinaryOp::*;
        matches!(self, Eq | Ne | Lt | Gt | Le | Ge)
    }
}
