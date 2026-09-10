//! Type representation (spec §5).

use std::rc::Rc;

use dz_resolve::SymbolId;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IntTy {
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
    Isize,
    Usize,
}

impl IntTy {
    pub fn name(&self) -> &'static str {
        match self {
            IntTy::I8 => "i8",
            IntTy::I16 => "i16",
            IntTy::I32 => "i32",
            IntTy::I64 => "i64",
            IntTy::U8 => "u8",
            IntTy::U16 => "u16",
            IntTy::U32 => "u32",
            IntTy::U64 => "u64",
            IntTy::Isize => "isize",
            IntTy::Usize => "usize",
        }
    }

    pub fn is_signed(&self) -> bool {
        matches!(self, IntTy::I8 | IntTy::I16 | IntTy::I32 | IntTy::I64 | IntTy::Isize)
    }

    /// Inclusive range of representable values, using `i128` so `u64` fits.
    pub fn range(&self) -> (i128, i128) {
        match self {
            IntTy::I8 => (i8::MIN as i128, i8::MAX as i128),
            IntTy::I16 => (i16::MIN as i128, i16::MAX as i128),
            IntTy::I32 => (i32::MIN as i128, i32::MAX as i128),
            // `isize`/`usize` are pointer-sized; 64-bit is the only target so far.
            IntTy::I64 | IntTy::Isize => (i64::MIN as i128, i64::MAX as i128),
            IntTy::U8 => (0, u8::MAX as i128),
            IntTy::U16 => (0, u16::MAX as i128),
            IntTy::U32 => (0, u32::MAX as i128),
            IntTy::U64 | IntTy::Usize => (0, u64::MAX as i128),
        }
    }

    pub fn from_name(name: &str) -> Option<IntTy> {
        Some(match name {
            "i8" => IntTy::I8,
            "i16" => IntTy::I16,
            "i32" => IntTy::I32,
            "i64" => IntTy::I64,
            "u8" => IntTy::U8,
            "u16" => IntTy::U16,
            "u32" => IntTy::U32,
            "u64" => IntTy::U64,
            "isize" => IntTy::Isize,
            "usize" => IntTy::Usize,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FloatTy {
    F32,
    F64,
}

impl FloatTy {
    pub fn name(&self) -> &'static str {
        match self {
            FloatTy::F32 => "f32",
            FloatTy::F64 => "f64",
        }
    }
}

/// A struct the language provides rather than the stdlib (spec §9.2, §12.7).
///
/// They are not `Ty::Named`, because a `Named` is a `SymbolId` and these are declared
/// nowhere: synthesising fake symbols for them would put two kinds of thing in the symbol
/// table and make every consumer ask which it had. When the stdlib arrives (M12) they
/// become ordinary declarations and this enum goes away.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BuiltinTy {
    /// `Error` — what a `!->` call carries when it fails (spec §9.2).
    Error,
    /// `SourceLocation` — where an error or a panic came from (spec §12.7).
    SourceLocation,
}

impl BuiltinTy {
    pub fn name(&self) -> &'static str {
        match self {
            BuiltinTy::Error => "Error",
            BuiltinTy::SourceLocation => "SourceLocation",
        }
    }

    pub fn from_name(name: &str) -> Option<BuiltinTy> {
        match name {
            "Error" => Some(BuiltinTy::Error),
            "SourceLocation" => Some(BuiltinTy::SourceLocation),
            _ => None,
        }
    }

    /// The fields, in declaration order, exactly as spec §9.2 and §12.7 write them.
    pub fn fields(&self) -> Vec<(&'static str, Ty)> {
        match self {
            BuiltinTy::Error => vec![
                ("code", Ty::Int(IntTy::U32)),
                ("message", Ty::Str),
                (
                    "location",
                    Ty::Ptr(Rc::new(Ty::Builtin(BuiltinTy::SourceLocation))),
                ),
            ],
            BuiltinTy::SourceLocation => vec![
                ("file", Ty::CStr),
                ("function", Ty::CStr),
                ("line", Ty::Int(IntTy::U32)),
                ("column", Ty::Int(IntTy::U32)),
            ],
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Ty {
    /// A type that could not be determined. Already reported, and compatible with
    /// everything so one mistake does not cascade.
    Error,
    /// The type of an expression that never produces a value (`return`, `break`, `panic`).
    Never,
    Void,
    Bool,
    Char,
    Int(IntTy),
    Float(FloatTy),
    Str,
    CStr,
    /// An integer literal that has not met a typing context yet (spec §2.5).
    IntLit,
    /// A float literal that has not met a typing context yet.
    FloatLit,
    Ptr(Rc<Ty>),
    OwnPtr(Rc<Ty>),
    Ref {
        mutable: bool,
        inner: Rc<Ty>,
    },
    Optional(Rc<Ty>),
    Fallible(Rc<Ty>),
    /// `len == None` is a slice `[]T`; `Some(n)` is `[n]T`.
    Array {
        len: Option<u64>,
        elem: Rc<Ty>,
    },
    Tuple(Vec<Ty>),
    /// The value of `a..b`; iterating one yields the element type (spec §10.4).
    Range(Rc<Ty>),
    Fn {
        params: Vec<Ty>,
        ret: Rc<Ty>,
    },
    /// A struct, union, enum or trait.
    Named(SymbolId),
    /// A struct the language provides: `Error`, `SourceLocation` (spec §9.2, §12.7).
    Builtin(BuiltinTy),
    /// The payload of one union variant, by the union's symbol and the variant's
    /// position (spec §7.5). It is not a type anyone writes: it exists so that the
    /// binding in `case Circle(c)` has something whose `.radius` resolves.
    Variant(SymbolId, usize),
    /// `Self` inside a struct or trait body.
    SelfTy,
}

impl Ty {
    pub fn ptr(inner: Ty) -> Ty {
        Ty::Ptr(Rc::new(inner))
    }

    pub fn reference(mutable: bool, inner: Ty) -> Ty {
        Ty::Ref { mutable, inner: Rc::new(inner) }
    }

    pub fn optional(inner: Ty) -> Ty {
        Ty::Optional(Rc::new(inner))
    }

    pub fn fallible(inner: Ty) -> Ty {
        Ty::Fallible(Rc::new(inner))
    }

    pub fn is_error(&self) -> bool {
        matches!(self, Ty::Error)
    }

    /// `Never` and `Error` slot into any context without a second complaint.
    pub fn is_wildcard(&self) -> bool {
        matches!(self, Ty::Error | Ty::Never)
    }

    pub fn is_numeric(&self) -> bool {
        matches!(self, Ty::Int(_) | Ty::Float(_) | Ty::IntLit | Ty::FloatLit)
    }

    pub fn is_integral(&self) -> bool {
        matches!(self, Ty::Int(_) | Ty::IntLit)
    }

    pub fn is_untyped_literal(&self) -> bool {
        matches!(self, Ty::IntLit | Ty::FloatLit)
    }

    /// The type an untyped literal settles on with no context (spec §2.5).
    pub fn defaulted(&self) -> Ty {
        match self {
            Ty::IntLit => Ty::Int(IntTy::I32),
            Ty::FloatLit => Ty::Float(FloatTy::F64),
            other => other.clone(),
        }
    }

    /// Strip `&`, `&mut` and `^` so `.` can reach through them (spec §5.2).
    pub fn peeled(&self) -> &Ty {
        let mut t = self;
        loop {
            match t {
                Ty::Ref { inner, .. } | Ty::Ptr(inner) | Ty::OwnPtr(inner) => t = inner,
                _ => return t,
            }
        }
    }

    /// Element type of something indexable.
    pub fn element(&self) -> Option<&Ty> {
        match self.peeled() {
            Ty::Array { elem, .. } => Some(elem),
            Ty::Range(elem) => Some(elem),
            _ => None,
        }
    }

    pub fn named(&self) -> Option<SymbolId> {
        match self {
            Ty::Named(id) => Some(*id),
            _ => None,
        }
    }
}

/// Can a value of type `actual` be used where `expected` is required?
///
/// This is deliberately strict: there are **no implicit numeric conversions in either
/// direction, not even widening** (spec §5.1). The single exception is an untyped literal,
/// which takes on whatever numeric type the context asks for.
pub fn assignable(expected: &Ty, actual: &Ty) -> bool {
    if expected.is_wildcard() || actual.is_wildcard() {
        return true;
    }
    match (expected, actual) {
        // Untyped literals adopt the expected type.
        (Ty::Int(_), Ty::IntLit) | (Ty::IntLit, Ty::Int(_)) => true,
        (Ty::Float(_), Ty::FloatLit) | (Ty::FloatLit, Ty::Float(_)) => true,
        // An integer literal is a valid float literal: `let x: f32 = 1`.
        (Ty::Float(_), Ty::IntLit) | (Ty::FloatLit, Ty::IntLit) => true,
        (Ty::IntLit, Ty::IntLit) | (Ty::FloatLit, Ty::FloatLit) => true,

        // A `&mut T` may be passed where `&T` is wanted, but never the reverse.
        (
            Ty::Ref { mutable: false, inner: a },
            Ty::Ref { mutable: _, inner: b },
        ) => assignable(a, b),
        (Ty::Ref { mutable: true, inner: a }, Ty::Ref { mutable: true, inner: b }) => {
            assignable(a, b)
        }

        // `own ^T` is a `^T` that also carries release duty, so it satisfies `^T`.
        (Ty::Ptr(a), Ty::Ptr(b)) | (Ty::Ptr(a), Ty::OwnPtr(b)) | (Ty::OwnPtr(a), Ty::OwnPtr(b)) => {
            assignable(a, b)
        }
        // ...and the reverse is how ownership is *acquired*: `alloc` returns a plain
        // `^u8` (spec §9.8), and storing it in an `own ^u8` field is what makes the
        // struct responsible for it. Nothing weaker would let anything own anything.
        (Ty::OwnPtr(a), Ty::Ptr(b)) => assignable(a, b),
        // `&mut T` satisfies `^T`: giving up the non-null guarantee is safe, and spec
        // §5.2 writes exactly this (`let p: ^Player = &mut world.players[0]`). The
        // reverse is not allowed — a `^T` may be null — and neither is `&T`, which
        // would silently gain the right to mutate.
        (Ty::Ptr(a), Ty::Ref { mutable: true, inner: b }) => assignable(a, b),

        (Ty::Optional(a), Ty::Optional(b)) => assignable(a, b),
        (Ty::Fallible(a), Ty::Fallible(b)) => assignable(a, b),

        // A fixed array converts to a slice of the same element type.
        (Ty::Array { len: None, elem: a }, Ty::Array { elem: b, .. }) => assignable(a, b),
        (Ty::Array { len: Some(m), elem: a }, Ty::Array { len: Some(n), elem: b }) => {
            m == n && assignable(a, b)
        }

        (Ty::Range(a), Ty::Range(b)) => assignable(a, b),
        (Ty::Tuple(a), Ty::Tuple(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(x, y)| assignable(x, y))
        }
        (Ty::Fn { params: p1, ret: r1 }, Ty::Fn { params: p2, ret: r2 }) => {
            p1.len() == p2.len()
                && p1.iter().zip(p2).all(|(x, y)| assignable(x, y))
                && assignable(r1, r2)
        }
        _ => expected == actual,
    }
}

/// The type two branches of the same expression settle on, if they agree.
pub fn join(a: &Ty, b: &Ty) -> Option<Ty> {
    if matches!(a, Ty::Never) {
        return Some(b.clone());
    }
    if matches!(b, Ty::Never) {
        return Some(a.clone());
    }
    if a.is_error() || b.is_error() {
        return Some(Ty::Error);
    }
    if a == b {
        return Some(a.clone());
    }
    // One side still untyped: take the concrete one.
    if a.is_untyped_literal() && assignable(b, a) {
        return Some(b.clone());
    }
    if b.is_untyped_literal() && assignable(a, b) {
        return Some(a.clone());
    }
    None
}
