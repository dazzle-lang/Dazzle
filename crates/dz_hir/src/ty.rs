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
                // A reference, not a pointer: it points into static storage for the whole
                // run, so it can neither be null nor dangle — the one stored reference the
                // language has (spec §9.2, §18.30).
                (
                    "location",
                    Ty::Ref {
                        mutable: false,
                        inner: Rc::new(Ty::Builtin(BuiltinTy::SourceLocation)),
                    },
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

/// A generic parameter, as seen from **inside** the body that declares it (spec §8.1).
///
/// The whole point of definition-site generics is that a generic body is checked once,
/// against its bounds, so `T` has to be a type the checker can reason about — not a hole.
/// `Ty::Error` was standing in for one until M6, and because `Ty::Error` is compatible with
/// everything, generic bodies had to be skipped rather than checked.
///
/// Identity is [`Param::id`] and nothing else: two parameters written `T` in two different
/// declarations are different types. The name rides along only so a type can be rendered
/// for a diagnostic without a side table; bounds live in `dz_check`'s table, since only the
/// checker needs them.
#[derive(Clone, Debug)]
pub struct Param {
    pub id: u32,
    pub name: Rc<str>,
    /// `view T`, or a trait's `view type Item`: it may be given a view, so a value of it is
    /// placed as a reference is (spec §6.7, D-0093). Not part of the identity.
    pub view: bool,
}

impl PartialEq for Param {
    fn eq(&self, other: &Param) -> bool {
        self.id == other.id
    }
}

impl Eq for Param {}

/// An associated type named off a type: `T::Item`, where `T: Iterate` (spec §8.5).
///
/// While `on` is a generic parameter — or itself a projection of one — nobody has chosen
/// the type yet, and it stays a `Proj`: two projections of the same base through the same
/// trait are the same type, and otherwise all that is known of it is the bounds the trait
/// puts on the associated type. Once `on` is concrete, the implementation for it says what
/// the type is, and the checker's `normalize` replaces the projection with that. So no
/// projection survives monomorphization.
///
/// The trait is part of the identity, arguments included: a `T: Mul<f32> + Mul<Vec2>` has
/// two `Output`s, one per implementation (spec §18.29).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Proj {
    pub on: Ty,
    pub trait_sym: SymbolId,
    pub trait_args: Vec<Ty>,
    pub name: Rc<str>,
}

/// A function type's `in` and `kills` clauses (spec §6.7), as written, with the names of the
/// parameters they are rooted at: a function type's clauses are resolved where they are
/// read, as a signature's are (`dz_check::ties::fn_type_sig`). A function item's value
/// carries its own signature's clauses, which is how storing it checks against the type's.
#[derive(Clone, Debug)]
pub enum FnKills {
    /// No `kills` path is a promise to kill nothing; no `in` path, that the result is in
    /// its one view input, as a declaration's is.
    Known { names: Rc<[String]>, results: Rc<[dz_ast::TiePath]>, paths: Rc<[dz_ast::TiePath]> },
    /// A pointer to a C or C++ function, whose header says nothing: a call through it kills
    /// everything reachable through its `&mut` and `^mut` arguments.
    Unknown,
}

impl FnKills {
    /// The clauses of a function type that names none: it kills nothing.
    pub fn none() -> Self {
        FnKills::Known { names: Rc::from([]), results: Rc::from([]), paths: Rc::from([]) }
    }

    /// Whether a function promising `self` fits where `wider` is promised, for parameters
    /// `params`: it kills no more ([`FnKills::kills_within`]), and its result points into no
    /// more ([`FnKills::results_within`]).
    pub fn within(&self, wider: &FnKills, params: &[Ty]) -> bool {
        self.kills_within(wider, params) && self.results_within(wider, params)
    }

    /// Whether every path `self` kills is one `wider` names, or under one. Unknown kills
    /// fit only where nothing can be killed, a type with no `&mut` or `^mut` parameter.
    pub fn kills_within(&self, wider: &FnKills, params: &[Ty]) -> bool {
        match (self, wider) {
            (_, FnKills::Unknown) => true,
            (FnKills::Unknown, FnKills::Known { .. }) => !params.iter().any(|p| {
                matches!(p, Ty::Ref { mutable: true, .. } | Ty::Ptr { mutable: true, .. })
            }),
            (
                FnKills::Known { names: mine, paths: narrow, .. },
                FnKills::Known { names: theirs, paths: broad, .. },
            ) => covered(mine, narrow, theirs, broad),
        }
    }

    /// Whether everything `self`'s result may point into, by its `in` or by elision, is
    /// something `wider`'s may point into, or under it. Where either side cannot say (no
    /// `in` and other than one view input, or a pointer from C), it fits: that signature
    /// has its own error, or points into C's memory, which no tie names.
    pub fn results_within(&self, wider: &FnKills, params: &[Ty]) -> bool {
        let (
            FnKills::Known { names: mine, results: narrow, .. },
            FnKills::Known { names: theirs, results: broad, .. },
        ) = (self, wider)
        else {
            return true;
        };
        match (result_places(mine, narrow, params), result_places(theirs, broad, params)) {
            (Some(narrow), Some(broad)) => narrow.iter().all(|(at, steps)| {
                broad.iter().any(|(there, wide)| {
                    at == there
                        && wide.len() <= steps.len()
                        && wide.iter().zip(*steps).all(|(a, b)| same_step(a, b))
                })
            }),
            _ => true,
        }
    }
}

/// Whether every path in `narrow`, rooted at a name in `mine`, is under one in `broad`,
/// rooted at the same parameter's position in `theirs`.
fn covered(
    mine: &[String],
    narrow: &[dz_ast::TiePath],
    theirs: &[String],
    broad: &[dz_ast::TiePath],
) -> bool {
    narrow.iter().all(|p| {
        let at = mine.iter().position(|n| *n == p.root.name);
        broad.iter().any(|q| {
            at.is_some()
                && theirs.iter().position(|n| *n == q.root.name) == at
                && q.steps.len() <= p.steps.len()
                && q.steps.iter().zip(&p.steps).all(|(a, b)| same_step(a, b))
        })
    })
}

/// Where a result may point, as parameter positions and the steps below them: each `in`
/// path, or, with none written, the one parameter that is a view, as a whole (spec §6.7).
/// `None` when that cannot be said: a path naming no parameter, or no `in` and other than
/// one view input.
fn result_places<'a>(
    names: &[String],
    written: &'a [dz_ast::TiePath],
    params: &[Ty],
) -> Option<Vec<(usize, &'a [dz_ast::TieStep])>> {
    if !written.is_empty() {
        return written
            .iter()
            .map(|p| names.iter().position(|n| *n == p.root.name).map(|at| (at, &p.steps[..])))
            .collect();
    }
    let views: Vec<usize> = params
        .iter()
        .enumerate()
        .filter(|(_, t)| {
            matches!(t, Ty::Ref { .. } | Ty::Ptr { .. } | Ty::Slice { .. } | Ty::Str | Ty::CStr)
        })
        .map(|(i, _)| i)
        .collect();
    match views[..] {
        [one] => Some(vec![(one, &[][..])]),
        _ => None,
    }
}

fn same_step(a: &dz_ast::TieStep, b: &dz_ast::TieStep) -> bool {
    use dz_ast::TieStep::*;
    match (a, b) {
        (Field(x), Field(y)) | (Tie(x), Tie(y)) => x.name == y.name,
        (Elements(_), Elements(_)) => true,
        _ => false,
    }
}

/// Not part of a type's identity (see [`Ty::Fn`]).
impl PartialEq for FnKills {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for FnKills {}

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
    /// `const str`: a `str` whose bytes are in static storage — a literal's, a constant's —
    /// so nothing ever kills it (spec §5.3, §6.7). The same C++ type as `str`; a value of it
    /// reads as a `str` wherever a `const str` is not wanted.
    ConstStr,
    CStr,
    /// An integer literal that has not met a typing context yet (spec §2.5).
    IntLit,
    /// A float literal that has not met a typing context yet.
    FloatLit,
    /// `^T` and `^mut T`. Writability is in the type, as for a reference and a slice: a
    /// `^T` is only read through, a `^mut T` written through too (spec §5.2, §18.37).
    Ptr {
        mutable: bool,
        inner: Rc<Ty>,
    },
    /// `own ^T` and `own ^mut T`: a pointer its holder must release (spec §6.5).
    OwnPtr {
        mutable: bool,
        inner: Rc<Ty>,
    },
    Ref {
        mutable: bool,
        inner: Rc<Ty>,
    },
    Optional(Rc<Ty>),
    Fallible(Rc<Ty>),
    /// `[n]T` — a fixed-size value type (spec §5.3).
    Array {
        len: u64,
        elem: Rc<Ty>,
    },
    /// `[]T` and `[]mut T`. Writability is part of the type, exactly as it is for a
    /// reference: a `[]T`'s elements cannot be written however the slice is bound, and a
    /// `[]mut T` satisfies a `[]T` while the reverse does not (spec §5.3, §18.22).
    Slice {
        mutable: bool,
        elem: Rc<Ty>,
    },
    Tuple(Vec<Ty>),
    /// The value of `a..b`; iterating one yields the element type (spec §10.4).
    Range(Rc<Ty>),
    Fn {
        params: Vec<Ty>,
        ret: Rc<Ty>,
        /// The most a function stored in it may kill (spec §6.7). Not part of the type's
        /// identity, so it changes neither the C++ type nor an instance; [`assignable`] is
        /// where it counts.
        kills: FnKills,
    },
    /// A struct, union, enum or trait, with its type arguments (spec §8.1).
    ///
    /// `args` is empty for a non-generic declaration. It is part of the type's identity:
    /// `Buffer<f32>` and `Buffer<i32>` are two types, and §8.4 emits two C++ structs for
    /// them. Inside `Buffer<T>`'s own body the argument is `Ty::Param`, which is what
    /// monomorphization later substitutes.
    Named {
        sym: SymbolId,
        args: Vec<Ty>,
    },
    /// A struct the language provides: `Error`, `SourceLocation` (spec §9.2, §12.7).
    Builtin(BuiltinTy),
    /// The payload of one union variant, by the union's symbol and the variant's
    /// position (spec §7.5). It is not a type anyone writes: it exists so that the
    /// binding in `case Circle(c)` has something whose `.radius` resolves. The last part is
    /// the union's type arguments, so `case Some(s)` on a `Maybe<i32>` gives `s.value` the
    /// type `i32` (spec §8.4); empty for a union that is not generic.
    Variant(SymbolId, usize, Vec<Ty>),
    /// `Self` inside a struct or trait body.
    SelfTy,
    /// A generic parameter inside the body that declares it (spec §8.1). Replaced by a
    /// concrete type during monomorphization; never present in emitted code.
    Param(Param),
    /// `T::Item`: an associated type of a type nobody has chosen yet (spec §8.5). Like a
    /// parameter, never present in emitted code.
    Assoc(Rc<Proj>),
}

impl Ty {
    pub fn ptr(mutable: bool, inner: Ty) -> Ty {
        Ty::Ptr { mutable, inner: Rc::new(inner) }
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
                Ty::Ref { inner, .. } | Ty::Ptr { inner, .. } | Ty::OwnPtr { inner, .. } => {
                    t = inner
                }
                _ => return t,
            }
        }
    }

    /// Strip `&` and `&mut` only, leaving pointers. Indexing reaches through a reference to
    /// what it refers to, but a `^T` is itself the thing being indexed.
    pub fn peeled_refs(&self) -> &Ty {
        let mut t = self;
        while let Ty::Ref { inner, .. } = t {
            t = inner;
        }
        t
    }

    /// Element type of something indexable.
    pub fn element(&self) -> Option<&Ty> {
        match self.peeled() {
            Ty::Array { elem, .. } | Ty::Slice { elem, .. } => Some(elem),
            Ty::Range(elem) => Some(elem),
            _ => None,
        }
    }

    /// `[]mut T`, `&mut T` and `^mut T` grant writes to what they point at; `[]T`, `&T` and
    /// `^T` do not (spec §5.2, §5.3).
    pub fn grants_write(&self) -> Option<bool> {
        match self {
            Ty::Slice { mutable, .. }
            | Ty::Ref { mutable, .. }
            | Ty::Ptr { mutable, .. }
            | Ty::OwnPtr { mutable, .. } => Some(*mutable),
            _ => None,
        }
    }

    pub fn named(&self) -> Option<SymbolId> {
        match self {
            Ty::Named { sym, .. } => Some(*sym),
            _ => None,
        }
    }

    /// A declaration with no type arguments — the non-generic case, and the spelling most
    /// call sites want.
    pub fn plain(sym: SymbolId) -> Ty {
        Ty::Named { sym, args: Vec::new() }
    }

    /// Does this type mention a generic parameter anywhere inside it? A type that does not
    /// needs no substitution, which is what lets monomorphization skip most of the tree.
    pub fn has_params(&self) -> bool {
        match self {
            Ty::Param(_) => true,
            Ty::Assoc(p) => p.on.has_params() || p.trait_args.iter().any(Ty::has_params),
            Ty::Optional(i) | Ty::Fallible(i) | Ty::Range(i) => i.has_params(),
            Ty::Ref { inner, .. } | Ty::Ptr { inner, .. } | Ty::OwnPtr { inner, .. } => {
                inner.has_params()
            }
            Ty::Array { elem, .. } | Ty::Slice { elem, .. } => elem.has_params(),
            Ty::Tuple(parts) => parts.iter().any(Ty::has_params),
            Ty::Named { args, .. } | Ty::Variant(_, _, args) => args.iter().any(Ty::has_params),
            Ty::Fn { params, ret, .. } => params.iter().any(Ty::has_params) || ret.has_params(),
            _ => false,
        }
    }
}

/// Replace generic parameters by type, by parameter id (spec §8.4).
///
/// This is the one operation that turns a declaration's types into an instantiation's:
/// `Buffer<T>`'s field `m_data: ^T` becomes `^f32` for `Buffer<f32>`. It is used both by
/// the checker, to type a member access on an instantiation, and by monomorphization, to
/// build the concrete item that is emitted.
///
/// A parameter with no entry in `map` is left alone, which is what makes a *partial*
/// substitution work: inside `Buffer<T>`, a method's own `U` stays `U` while `T` is
/// replaced.
pub fn substitute(t: &Ty, map: &std::collections::HashMap<u32, Ty>) -> Ty {
    if map.is_empty() || !t.has_params() {
        return t.clone();
    }
    substitute_with(t, map, None)
}

/// [`substitute`], and additionally what `Self` stands for.
///
/// Both replacements walk the same tree, so they are one traversal rather than two: a
/// trait method's `fn eq(&self, rhs: &Self) -> bool`, read through a `T: Eq`, needs `Self`
/// to become `T` at the same moment the trait's own parameters become `T`'s arguments.
pub fn substitute_with(
    t: &Ty,
    map: &std::collections::HashMap<u32, Ty>,
    self_ty: Option<&Ty>,
) -> Ty {
    if map.is_empty() && self_ty.is_none() {
        return t.clone();
    }
    let rc = |x: &Rc<Ty>| Rc::new(substitute_with(x, map, self_ty));
    let substitute = |x: &Ty| substitute_with(x, map, self_ty);
    match t {
        Ty::SelfTy => self_ty.cloned().unwrap_or(Ty::SelfTy),
        Ty::Param(p) => map.get(&p.id).cloned().unwrap_or_else(|| t.clone()),
        Ty::Ptr { mutable, inner } => Ty::Ptr { mutable: *mutable, inner: rc(inner) },
        Ty::OwnPtr { mutable, inner } => Ty::OwnPtr { mutable: *mutable, inner: rc(inner) },
        Ty::Ref { mutable, inner } => Ty::Ref { mutable: *mutable, inner: rc(inner) },
        Ty::Optional(i) => Ty::Optional(rc(i)),
        Ty::Fallible(i) => Ty::Fallible(rc(i)),
        Ty::Range(i) => Ty::Range(rc(i)),
        Ty::Array { len, elem } => Ty::Array { len: *len, elem: rc(elem) },
        Ty::Slice { mutable, elem } => Ty::Slice { mutable: *mutable, elem: rc(elem) },
        Ty::Tuple(parts) => Ty::Tuple(parts.iter().map(&substitute).collect()),
        Ty::Named { sym, args } => {
            Ty::Named { sym: *sym, args: args.iter().map(&substitute).collect() }
        }
        Ty::Variant(sym, index, args) => {
            Ty::Variant(*sym, *index, args.iter().map(&substitute).collect())
        }
        Ty::Fn { params, ret, kills } => Ty::Fn {
            params: params.iter().map(&substitute).collect(),
            ret: rc(ret),
            kills: kills.clone(),
        },
        // The base may become concrete here, and then the projection has an answer — but
        // only the checker's type table knows the implementations, so finding it is
        // `TypeTable::normalize`'s job, run after this.
        Ty::Assoc(p) => Ty::Assoc(Rc::new(Proj {
            on: substitute(&p.on),
            trait_sym: p.trait_sym,
            trait_args: p.trait_args.iter().map(&substitute).collect(),
            name: p.name.clone(),
        })),
        other => other.clone(),
    }
}

/// Does `t` contain a projection anywhere? A type that does not is already normal, which
/// lets the checker skip the walk for almost every type it sees.
pub fn has_assoc(t: &Ty) -> bool {
    match t {
        Ty::Assoc(_) => true,
        Ty::Optional(i) | Ty::Fallible(i) | Ty::Range(i) => has_assoc(i),
        Ty::Ref { inner, .. } | Ty::Ptr { inner, .. } | Ty::OwnPtr { inner, .. } => {
            has_assoc(inner)
        }
        Ty::Array { elem, .. } | Ty::Slice { elem, .. } => has_assoc(elem),
        Ty::Tuple(parts) => parts.iter().any(has_assoc),
        Ty::Named { args, .. } | Ty::Variant(_, _, args) => args.iter().any(has_assoc),
        Ty::Fn { params, ret, .. } => params.iter().any(has_assoc) || has_assoc(ret),
        _ => false,
    }
}

/// `t` with `f` applied to each type directly inside it, and its own shape kept. A
/// projection's parts are left alone: what to do with one is the caller's question.
pub fn map_parts(t: &Ty, f: &dyn Fn(&Ty) -> Ty) -> Ty {
    let rc = |x: &Rc<Ty>| Rc::new(f(x));
    match t {
        Ty::Ptr { mutable, inner } => Ty::Ptr { mutable: *mutable, inner: rc(inner) },
        Ty::OwnPtr { mutable, inner } => Ty::OwnPtr { mutable: *mutable, inner: rc(inner) },
        Ty::Ref { mutable, inner } => Ty::Ref { mutable: *mutable, inner: rc(inner) },
        Ty::Optional(i) => Ty::Optional(rc(i)),
        Ty::Fallible(i) => Ty::Fallible(rc(i)),
        Ty::Range(i) => Ty::Range(rc(i)),
        Ty::Array { len, elem } => Ty::Array { len: *len, elem: rc(elem) },
        Ty::Slice { mutable, elem } => Ty::Slice { mutable: *mutable, elem: rc(elem) },
        Ty::Tuple(parts) => Ty::Tuple(parts.iter().map(f).collect()),
        Ty::Named { sym, args } => Ty::Named { sym: *sym, args: args.iter().map(f).collect() },
        Ty::Variant(sym, index, args) => Ty::Variant(*sym, *index, args.iter().map(f).collect()),
        Ty::Fn { params, ret, kills } => {
            Ty::Fn { params: params.iter().map(f).collect(), ret: rc(ret), kills: kills.clone() }
        }
        other => other.clone(),
    }
}

/// Is this a type nobody has chosen yet — a generic parameter, or an associated type of
/// one (spec §8.1, §8.5)? What such a type can do is exactly what its bounds say.
pub fn is_abstract(t: &Ty) -> bool {
    match t {
        Ty::Param(_) => true,
        Ty::Assoc(p) => is_abstract(&p.on),
        _ => false,
    }
}

/// Replace every occurrence of the type `from` inside `t` with `to`.
///
/// Used where a type stands in for another by position rather than by parameter: inside a
/// trait's own default body the checker reads `self` through the trait's signature, so
/// `Self` there is spelled as the trait, and an implementer's copy has to say itself.
pub fn replace(t: &Ty, from: &Ty, to: &Ty) -> Ty {
    if t == from {
        return to.clone();
    }
    let rc = |x: &Rc<Ty>| Rc::new(replace(x, from, to));
    let r = |x: &Ty| replace(x, from, to);
    match t {
        Ty::Ptr { mutable, inner } => Ty::Ptr { mutable: *mutable, inner: rc(inner) },
        Ty::OwnPtr { mutable, inner } => Ty::OwnPtr { mutable: *mutable, inner: rc(inner) },
        Ty::Ref { mutable, inner } => Ty::Ref { mutable: *mutable, inner: rc(inner) },
        Ty::Optional(i) => Ty::Optional(rc(i)),
        Ty::Fallible(i) => Ty::Fallible(rc(i)),
        Ty::Range(i) => Ty::Range(rc(i)),
        Ty::Array { len, elem } => Ty::Array { len: *len, elem: rc(elem) },
        Ty::Slice { mutable, elem } => Ty::Slice { mutable: *mutable, elem: rc(elem) },
        Ty::Tuple(parts) => Ty::Tuple(parts.iter().map(r).collect()),
        Ty::Named { sym, args } => Ty::Named { sym: *sym, args: args.iter().map(r).collect() },
        Ty::Variant(sym, index, args) => Ty::Variant(*sym, *index, args.iter().map(r).collect()),
        Ty::Fn { params, ret, kills } => {
            Ty::Fn { params: params.iter().map(r).collect(), ret: rc(ret), kills: kills.clone() }
        }
        Ty::Assoc(p) => Ty::Assoc(Rc::new(Proj {
            on: replace(&p.on, from, to),
            trait_sym: p.trait_sym,
            trait_args: p.trait_args.iter().map(r).collect(),
            name: p.name.clone(),
        })),
        other => other.clone(),
    }
}

/// The pointee half of a pointer conversion. Writing needs a writable source. Through a
/// read-only pointer the pointee may itself widen — `^^mut T` satisfies `^^T` — but through
/// a writable one it must match both ways: a `^mut ^T` made from a `^mut ^mut T` could store
/// a read-only pointer where a writable one is expected. C++ rejects `T**` → `const T**` for
/// the same reason, and the output has to compile.
fn pointee_assignable(want_mut: bool, have_mut: bool, want: &Ty, have: &Ty) -> bool {
    if want_mut {
        have_mut && assignable(want, have) && assignable(have, want)
    } else {
        assignable(want, have)
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
        (Ty::Ref { mutable: false, inner: a }, Ty::Ref { mutable: _, inner: b }) => {
            assignable(a, b)
        }
        (Ty::Ref { mutable: true, inner: a }, Ty::Ref { mutable: true, inner: b }) => {
            assignable(a, b)
        }

        // Pointers follow references (spec §5.2, §18.37): a writable one satisfies a
        // read-only one, never the reverse. `own` is release duty rather than a different
        // pointer, so an `own ^T` satisfies a `^T` — and a `^T` satisfies an `own ^T`, which
        // is how ownership is *acquired*: `alloc` returns a `^mut u8` (spec §9.8), and
        // storing it in an `own ^mut u8` field is what makes the struct responsible for it.
        (
            Ty::Ptr { mutable: want, inner: a } | Ty::OwnPtr { mutable: want, inner: a },
            Ty::Ptr { mutable: have, inner: b } | Ty::OwnPtr { mutable: have, inner: b },
        ) => pointee_assignable(*want, *have, a, b),
        // A reference satisfies a pointer of no more writability: giving up the non-null
        // guarantee is safe, and §5.2 writes exactly this (`let p: ^mut Player = &mut
        // world.players[0]`). The reverse is not allowed — a pointer may be null — and goes
        // through `or` instead.
        (Ty::Ptr { mutable: want, inner: a }, Ty::Ref { mutable: have, inner: b }) => {
            pointee_assignable(*want, *have, a, b)
        }

        (Ty::Optional(a), Ty::Optional(b)) => assignable(a, b),
        // A `T` is the success case of a `T?` wherever one is wanted, as at `return` (spec
        // §9.1, 18.53). Never the reverse: a `T?` becomes a `T` only through `or`.
        (Ty::Optional(a), b) => assignable(a, b),
        (Ty::Fallible(a), Ty::Fallible(b)) => assignable(a, b),

        (Ty::Array { len: m, elem: a }, Ty::Array { len: n, elem: b }) => {
            m == n && assignable(a, b)
        }
        // A `[]mut T` satisfies a `[]T`, exactly as `&mut T` satisfies `&T`; the reverse
        // would silently gain the right to write (spec §5.3, §18.22).
        (Ty::Slice { mutable: false, elem: a }, Ty::Slice { elem: b, .. }) => assignable(a, b),
        (Ty::Slice { mutable: true, elem: a }, Ty::Slice { mutable: true, elem: b }) => {
            assignable(a, b)
        }
        // A fixed array converts to a view of its elements. Producing a `[]mut T` this way
        // additionally needs the array's *place* to be writable, which is not a question a
        // type can answer — the caller checks it (see `coerce_to_slice` in `dz_check`).
        (Ty::Slice { elem: a, .. }, Ty::Array { elem: b, .. }) => assignable(a, b),

        (Ty::Range(a), Ty::Range(b)) => assignable(a, b),
        // Static text is text; text is not static (spec §5.3).
        (Ty::Str, Ty::ConstStr) => true,
        (Ty::Tuple(a), Ty::Tuple(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(x, y)| assignable(x, y))
        }
        // A function fits a function type that promises at least what it kills (§6.7).
        (Ty::Fn { params: p1, ret: r1, kills: k1 }, Ty::Fn { params: p2, ret: r2, kills: k2 }) => {
            p1.len() == p2.len()
                && p1.iter().zip(p2).all(|(x, y)| assignable(x, y))
                && assignable(r1, r2)
                && k2.within(k1, p1)
        }
        _ => expected == actual,
    }
}

/// The placeholder type of a `none` arm whose `T?` is not known until its `if` or
/// `switch` joins the arms: an optional of nothing (spec §9.1, 18.53). The checker replaces
/// it before the body is finished, or reports that the `none` has no type.
pub fn pending_none() -> Ty {
    Ty::Optional(Rc::new(Ty::Never))
}

pub fn is_pending_none(t: &Ty) -> bool {
    matches!(t, Ty::Optional(inner) if matches!(**inner, Ty::Never))
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
    // A `none` arm still waiting for its type takes the other arm's (spec §9.1, 18.53).
    for (pending, other) in [(a, b), (b, a)] {
        if is_pending_none(pending) {
            return match other {
                Ty::Void => None,
                Ty::Optional(_) => Some(other.clone()),
                _ => Some(Ty::Optional(Rc::new(other.defaulted()))),
            };
        }
    }
    // One side still untyped: take the concrete one.
    if a.is_untyped_literal() && assignable(b, a) {
        return Some(b.clone());
    }
    if b.is_untyped_literal() && assignable(a, b) {
        return Some(a.clone());
    }
    // `if alive { 100 } else { none }`: an arm that is a `T` joins one that is a `T?`, and
    // the whole is a `T?` (spec §9.1, 18.53).
    if let Ty::Optional(inner) = a {
        if !matches!(b, Ty::Optional(_)) && assignable(inner, b) {
            return Some(a.clone());
        }
    }
    if let Ty::Optional(inner) = b {
        if !matches!(a, Ty::Optional(_)) && assignable(inner, a) {
            return Some(b.clone());
        }
    }
    None
}
