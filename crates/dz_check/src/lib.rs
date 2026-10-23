//! The type checker (spec §5, §7, §9, §10, §11).
//!
//! M3's remit is a **non-generic** program. Signatures are collected for everything, then
//! every non-generic function body is checked.
//!
//! Deliberately deferred, each to the milestone that owns it:
//!
//! * generics and trait conformance — M6 (spec §8);
//! * ownership, moves and definite return — M5 (spec §6);
//! * macro bodies — M10; C++ imports — M7.
//!
//! Checking also **builds the HIR** (spec §17.1, `dz_hir`): every rule returns the type it
//! concluded together with the node it built for it. That is deliberate — the types this
//! pass computes are exactly the ones the HIR stores, and a second pass to collect them
//! would recompute them and could disagree. See `build.rs`.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use dz_ast::*;
use dz_diag::{codes, Diagnostic, Diagnostics, Edit};
use dz_hir::Hir;
use dz_project::{ModuleId, Workspace};
use dz_resolve::{ParsedFile, Resolution, SymbolId, SymbolKind};
use dz_span::{FileId, SourceMap, Span};

mod build;
mod check;
mod impls;

/// The type model lives in `dz_hir`, since the HIR stores a `Ty` on every node and
/// `dz_hir` sits below this crate. Re-exported so `dz_check::ty::Ty` keeps working.
pub use dz_hir::ty;

use check::plural;
use ty::{FloatTy, IntTy, Ty};

// ---------------------------------------------------------------------------
// signatures
// ---------------------------------------------------------------------------

/// One generic parameter of a declaration, with the bounds its body is checked against
/// (spec §8.1).
///
/// The bounds are the *whole* of what the body may assume. `a > b` inside `fn max<T: Ord>`
/// is legal because `Ord` is here; `a.frobnicate()` is an error reported at that line, not
/// at some instantiation — which is the entire reason for definition-site generics (§8.6).
#[derive(Clone, Debug)]
pub struct ParamDef {
    pub param: ty::Param,
    /// Empty is the unbounded case (§8.2): `T` may then only be moved,
    /// default-initialised, dropped, and queried with `T::`.
    pub bounds: Vec<TraitRef>,
    /// `Rhs = Self` — what an omitted argument means (spec §11.2). Lowered in the
    /// declaration's own scope, so `Self` here is `Ty::SelfTy` and is substituted by
    /// whichever type the trait is later applied to.
    pub default: Option<Ty>,
    pub span: Span,
}

impl ParamDef {
    pub fn name(&self) -> &str {
        &self.param.name
    }
}

/// One trait applied to arguments: `Ord`, `Mul<f32>`, `Iterate` (spec §7.6, §11.2).
///
/// The arguments matter. `Mul<f32>` and `Mul<Vec3>` are different obligations, and reading
/// `mul`'s signature off the first has to give `fn mul(&self, rhs: &f32) -> Output` — so a
/// bare `SymbolId` is not enough to say what a bound or an implementation means.
#[derive(Clone, Debug)]
pub struct TraitRef {
    pub trait_sym: SymbolId,
    /// Positional, already filled in from the trait's parameter defaults where the source
    /// left them out, so this is always as long as the trait's parameter list.
    pub args: Vec<Ty>,
    pub span: Span,
}

impl TraitRef {
    /// The map that turns a type written inside the trait into one seen through this
    /// reference. `self_ty` is what the trait is applied to, which is what `Self` means.
    pub(crate) fn subst_map(&self, params: &[ParamDef]) -> HashMap<u32, Ty> {
        params.iter().map(|p| p.param.id).zip(self.args.iter().cloned()).collect()
    }
}

#[derive(Clone, Debug)]
pub struct FieldSig {
    pub name: String,
    pub ty: Ty,
    pub vis: Vis,
    pub has_default: bool,
    pub span: Span,
    /// The base this field was inherited from, if it was — `None` for a type's own.
    /// `span` points into the base's declaration, which is in the base's file, so anything
    /// that turns a field into a location has to ask this and not the type it was reached
    /// through (spec §7.3, 18.32).
    pub declared_in: Option<SymbolId>,
    /// An imported C bit-field, `unsigned flags : 3`. Read and written like any other
    /// member; its address cannot be taken, in C++ or here (spec §15.2).
    pub bitfield: bool,
}

#[derive(Clone, Debug)]
pub struct ParamSig {
    pub name: String,
    pub ty: Ty,
    pub has_default: bool,
    /// `loc: SourceLocation = #location`: the intrinsic a default names, which is evaluated
    /// at the caller and so is spelled wherever the function is declared — including on a
    /// trait object's forwarding members (spec §12.7).
    pub caller_default: Option<String>,
    /// The C spelling an imported parameter is cast to at the call: `char *` is not
    /// `int8_t *`, nor `long *` an `int32_t *`, and pointers do not convert (spec §15.2).
    pub c_cast: Option<String>,
}

/// The intrinsic a parameter's default is, when it is one the caller supplies (spec §12.7).
pub fn caller_default(default: Option<&Expr>) -> Option<String> {
    match &default?.kind {
        ExprKind::Intrinsic { name, args }
            if args.is_empty()
                && matches!(
                    name.name.as_str(),
                    "location" | "line" | "column" | "file" | "function"
                ) =>
        {
            Some(name.name.clone())
        }
        _ => None,
    }
}

#[derive(Clone, Debug)]
pub struct FnSig {
    pub params: Vec<ParamSig>,
    /// The **success** type. A fallible signature's failure is not part of it (spec §9).
    pub ret: Ty,
    /// `!->`: calling this may fail, so the call must be consumed by an `or` clause in the
    /// statement it appears in (spec §9.7).
    pub fallible: bool,
    /// `None` for a free function or a constructor.
    pub takes_self: Option<bool>,
    pub is_mut: bool,
    /// `unsafe fn`: a call needs an `unsafe` block, and the body is one (spec §5.2).
    pub is_unsafe: bool,
    pub vis: Vis,
    pub span: Span,
    /// `fn max<T: Ord>` — this function's own generic parameters (spec §8.1). A method of
    /// a generic struct does **not** repeat the struct's parameters here; those are on the
    /// struct, and both are in scope inside the method.
    pub generics: Vec<ParamDef>,
    /// Whether a body was written. Only interesting on a trait method, where no body means
    /// every implementer must supply one and a body makes it a default method (spec §7.6).
    pub has_body: bool,
    /// An imported function whose returned pointer is cast back to Dazzle's own C++
    /// type — the other half of [`ParamSig::c_cast`] (spec §15.2).
    pub ret_cast: bool,
    /// `virtual fn` / `override fn` (spec §7.3, 18.32). A `virtual` with `has_body`
    /// false is abstract, and makes its struct not constructible.
    pub dispatch: Dispatch,
    /// The index of the `..T` parameter, if there is one (spec §7.2): its `ty` is the `[]T`
    /// the body sees, and a call gives it any number of `T`s — or one slice spread with
    /// `..xs`. Any parameters after it are passed by name.
    pub variadic: Option<usize>,
}

impl FnSig {
    /// The element type of the variadic parameter, if there is one.
    pub fn variadic_elem(&self) -> Option<&Ty> {
        match &self.params.get(self.variadic?)?.ty {
            Ty::Slice { elem, .. } => Some(elem),
            _ => None,
        }
    }

    /// Parameters that must be supplied, i.e. those without a default (spec §7.2).
    pub fn required(&self) -> usize {
        self.params.iter().filter(|p| !p.has_default).count()
    }

    pub fn is_generic(&self) -> bool {
        !self.generics.is_empty()
    }
}

#[derive(Clone, Debug, Default)]
pub struct StructSig {
    /// The base's fields first, then this type's own — so a `&Derived` is already a
    /// `&Base` at offset zero, and the C++ output can be real single inheritance
    /// (spec §7.3, 18.32). `inherited` says where the type's own begin, and every field
    /// index in the HIR is an index into this one flat list.
    pub fields: Vec<FieldSig>,
    /// `struct Button : UiElement` — the one struct this one inherits from (18.32).
    pub base: Option<SymbolId>,
    /// The base's type arguments, `[i32]` for `struct Button : Panel<i32>` — written in
    /// terms of this type's own parameters, `[T]` for `struct Slider<T> : Panel<T>`. Empty
    /// for a base that is not generic (T-0035).
    pub base_args: Vec<Ty>,
    /// How many leading entries of `fields` came from bases. Zero without a base.
    pub inherited: usize,
    /// Does this type, or anything it inherits from, declare a `virtual` method? A C++
    /// class with one is never trivially copyable and needs a virtual destructor, so this
    /// is asked well away from the declaration that caused it.
    pub polymorphic: bool,
    /// An imported C struct whose header declares it and never defines it — a handle
    /// (spec §15.2, §18.40). No fields to name, not copyable, never used by value.
    pub opaque: bool,
    /// An imported C `union`: its members are one piece of memory rather than fields beside
    /// each other, so a literal may set only one of them (spec §15.2, §18.41).
    pub is_union: bool,
    /// An imported C++ class with a destructor of its own. C++ runs it, so Dazzle emits no
    /// `drop` for one — but the type is move-only all the same, because copying it would
    /// run that destructor twice (spec §15.2, §6.3).
    pub cpp_owned: bool,
    /// Keyed by method key: the name itself, or `name#1`, `name#2`… for a second and later
    /// method of the same name, which is allowed only for implementations of one trait at
    /// different arguments (spec §18.27).
    pub methods: HashMap<String, FnSig>,
    /// The keys of the `fn` members, in declaration order.
    pub method_keys: Vec<String>,
    /// The keys sharing each method name, first-declared first.
    pub overloads: HashMap<String, Vec<String>>,
    pub ctors: Vec<FnSig>,
    /// `copy(other: &Self)` — its presence is what makes an otherwise move-only type
    /// copyable (spec §6.3). The signature is kept for the parameter's span.
    pub copy: Option<FnSig>,
    /// `drop()` — its presence makes the type move-only and gives it a C++ destructor
    /// (spec §6.3, §6.6).
    pub drop: Option<FnSig>,
    /// `type Item` on a trait, `type Item = ^T` on an implementer (spec §8.5).
    ///
    /// `None` is a trait naming a type it does not choose; `Some` is an implementer
    /// choosing one. That is the whole distinction between the two, so one map serves both
    /// and conformance is "every `None` on the trait has a `Some` on the implementer".
    pub assoc_types: Vec<AssocType>,
    /// `const Global: ^mut Allocator = …` declared in the type (spec §7.3).
    pub consts: Vec<AssocConst>,
}

/// A constant named through its type: `Allocators::Global` (spec §3.3, §7.3).
#[derive(Clone, Debug)]
pub struct AssocConst {
    pub name: String,
    pub ty: Ty,
    pub vis: Vis,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct AssocType {
    pub name: String,
    pub ty: Option<Ty>,
    /// `type Item : Format` — what an implementer's choice must satisfy (spec §8.5).
    pub bounds: Vec<TraitRef>,
    pub span: Span,
}

/// One `impl Trait for Type` (spec §7.6). An implementation has no name, so it is addressed
/// by its position in [`TypeTable::impls`].
///
/// The trait's arguments and the type may both mention the implementation's own parameters:
/// `impl<T: Copy> Copy for Buffer<T>` has `self_ty` `Buffer<T>` with `T` among `generics`,
/// and applies to `Buffer<f32>` by matching `T` to `f32` and checking `f32: Copy`.
#[derive(Clone, Debug)]
pub struct ImplSig {
    pub trait_ref: TraitRef,
    pub self_ty: Ty,
    pub generics: Vec<ParamDef>,
    /// `Self` already replaced by `self_ty`.
    pub methods: HashMap<String, FnSig>,
    /// The methods in declaration order, so everything built from them is deterministic.
    pub method_order: Vec<String>,
    /// This implementation's choices — which is why `Mat4 × Vec4` and `Mat4 × f32` can
    /// each have their own `Output` (spec §8.5, §18.29).
    pub assoc_types: Vec<AssocType>,
    pub module: ModuleId,
    pub file: FileId,
    /// The trait as written, which is what diagnostics about the implementation point at.
    pub span: Span,
    /// The whole `impl … { … }`, braces included. `span` points at the trait name, which is
    /// where a reader should look; this is where a *fix* writes a method that is missing.
    pub block_span: Span,
}

#[derive(Clone, Debug)]
pub struct VariantSig {
    pub name: String,
    pub fields: Vec<FieldSig>,
    pub span: Span,
    /// A bare union variant names an existing struct and carries a value of it, so its
    /// payload is that struct — its fields, defaults and methods — rather than a field list
    /// of the variant's own, and `fields` is empty (spec §7.5, 18.45).
    pub carries: Option<SymbolId>,
}

#[derive(Clone, Debug)]
pub enum ItemSig {
    Struct(StructSig),
    Union {
        variants: Vec<VariantSig>,
        /// Its methods (spec §7.5, 18.78), kept as a struct's are so that a method call
        /// is resolved the same way on either. Nothing else in it is ever filled in.
        methods: StructSig,
    },
    Enum {
        backing: IntTy,
        variants: Vec<VariantSig>,
        /// The `= n` a variant was given, by position. `None` means "one more than the
        /// previous", which C++ spells the same way, so codegen prints nothing for it.
        values: Vec<Option<i128>>,
    },
    Trait(StructSig),
    Fn(FnSig),
    Const(Ty),
    /// A `static` and its declared type (spec §7.7).
    Static(Ty),
}

/// Everything the body checker needs to know about declarations.
#[derive(Clone)]
pub struct TypeTable {
    /// Every `#[test] fn`, in declaration order across the workspace (spec §19 M14).
    /// Held here because both halves of the build ask about it: a normal build leaves
    /// these functions out entirely, and `dz test` generates an entry point that is
    /// nothing but calls to them.
    pub tests: Vec<SymbolId>,
    /// Every `#[bench] fn`, the same way (spec §19 M15). Separate from `tests` because
    /// the two are run by different commands and neither belongs in the other's binary.
    pub benches: Vec<SymbolId>,
    sigs: HashMap<SymbolId, ItemSig>,
    /// The generic parameters of a struct, union or trait, in declaration order (spec
    /// §8.1). A generic *function*'s parameters live on its [`FnSig`] instead, because a
    /// method has a signature but no `SymbolId` of its own.
    type_params: HashMap<SymbolId, Vec<ParamDef>>,
    /// Bounds by parameter id, so a `Ty::Param` reached from anywhere can be asked what it
    /// is allowed to do.
    param_bounds: HashMap<u32, Vec<TraitRef>>,
    /// A trait's associated types, as parameters (spec §8.5). Inside the trait `Item` is a
    /// type nobody has chosen; reading a method through an implementer substitutes each of
    /// these with whatever that implementer chose.
    assoc_params: HashMap<SymbolId, Vec<(String, ty::Param)>>,
    /// Every declaration's associated types, resolved. Collected before signatures, because
    /// a method may return one and be written above it.
    assoc_decls: HashMap<SymbolId, Vec<AssocType>>,
    /// A trait's supertraits, `trait Ord : Eq` (spec §7.6).
    declared_traits: HashMap<SymbolId, Vec<TraitRef>>,
    /// The prelude's `Copy`, which every copyable type satisfies (spec §8.2). Held here
    /// because copyability is asked from every phase.
    copy_trait: Option<SymbolId>,
    /// The prelude's `Drop`, for the same reason — and so that inheriting a base's
    /// implementations can leave the two special members alone (spec §6.1, §7.3).
    drop_trait: Option<SymbolId>,
    /// Every `impl Trait for Type`, in declaration order (spec §7.6).
    impls: Vec<ImplSig>,
    /// The `impl Drop` / `impl Copy` of each struct that has one (spec §6.1). They are
    /// implementations like any other, but they become the struct's C++ destructor and
    /// copy constructor, so each struct has at most one of each and they are found by struct.
    drop_impls: HashMap<SymbolId, usize>,
    copy_impls: HashMap<SymbolId, usize>,
    /// The standard library's traits by name, for the ones the primitives implement
    /// (spec §11.2, §14.4).
    prelude_names: HashMap<SymbolId, String>,
}

impl TypeTable {
    pub fn get(&self, id: SymbolId) -> Option<&ItemSig> {
        self.sigs.get(&id)
    }

    /// The generic parameters of a type declaration, empty if it has none.
    pub fn type_params(&self, id: SymbolId) -> &[ParamDef] {
        self.type_params.get(&id).map_or(&[], |v| v.as_slice())
    }

    /// What a generic parameter is allowed to do (spec §8.1). Empty means §8.2's
    /// unbounded case, not "unknown".
    pub fn bounds_of(&self, p: &ty::Param) -> &[TraitRef] {
        self.param_bounds.get(&p.id).map_or(&[], |v| v.as_slice())
    }

    /// `trait Ord : Eq` — the traits a trait requires (spec §7.6).
    pub fn supertraits(&self, t: SymbolId) -> &[TraitRef] {
        self.declared_traits.get(&t).map_or(&[], |v| v.as_slice())
    }

    /// Every trait obligation implied by `roots`, supertraits included (spec §7.6).
    ///
    /// A supertrait's arguments are written in terms of its *subtrait's* parameters, so
    /// `trait Pair<A> : Eq<A>` reached through `Pair<i32>` yields `Eq<i32>` and not `Eq<A>`.
    /// Substituting on the way down is the whole reason this is not a plain graph walk.
    /// `self_ty` is what the obligations are on, which is what `Self` means throughout.
    pub fn trait_closure(&self, roots: &[TraitRef], self_ty: &Ty) -> Vec<TraitRef> {
        let mut out: Vec<TraitRef> = Vec::new();
        let mut queue: Vec<TraitRef> = roots.to_vec();
        while let Some(r) = queue.pop() {
            if out.iter().any(|x| x.trait_sym == r.trait_sym && x.args == r.args) {
                continue;
            }
            let map = r.subst_map(self.type_params(r.trait_sym));
            for s in self.supertraits(r.trait_sym) {
                queue.push(TraitRef {
                    trait_sym: s.trait_sym,
                    args: s
                        .args
                        .iter()
                        .map(|a| ty::substitute_with(a, &map, Some(self_ty)))
                        .collect(),
                    span: r.span,
                });
            }
            out.push(r);
        }
        out
    }

    /// A trait's associated types, as parameters (spec §8.5).
    pub fn assoc_params(&self, t: SymbolId) -> &[(String, ty::Param)] {
        self.assoc_params.get(&t).map_or(&[], |v| v.as_slice())
    }

    /// What the implementation of `r` for `self_ty` chose for each of the trait's
    /// associated types (spec §8.5).
    ///
    /// Each implementation chooses its own, so `Mat4`'s `Mul<Vec4>` and `Mul<f32>` can
    /// answer `Output` differently (§18.29). A `Ty::Param` has no implementation — the bound
    /// says `T: Iterate` without saying what `T::Item` is — so its associated types stay
    /// themselves, which is exactly right for a body checked once at its definition.
    pub fn assoc_bindings(&self, r: &TraitRef, self_ty: &Ty) -> HashMap<u32, Ty> {
        let mut map = HashMap::new();
        // Read through a bound, `T: Iterate`, nothing has chosen `Item` yet: it is the
        // projection `T::Item`, which is one type wherever it is reached from `T` and
        // becomes the implementation's choice once `T` is known (spec §8.5).
        if ty::is_abstract(self_ty) {
            for (name, param) in self.assoc_params(r.trait_sym) {
                map.insert(
                    param.id,
                    Ty::Assoc(Rc::new(ty::Proj {
                        on: self_ty.clone(),
                        trait_sym: r.trait_sym,
                        trait_args: r.args.clone(),
                        name: Rc::from(name.as_str()),
                    })),
                );
            }
            return map;
        }
        let Some((idx, imap)) = self.impl_of(r, self_ty) else { return map };
        let imp = &self.impls[idx];
        for (name, param) in self.assoc_params(r.trait_sym) {
            if let Some(chosen) = imp.assoc_types.iter().find(|a| &a.name == name) {
                if let Some(ty) = &chosen.ty {
                    map.insert(param.id, self.subst_with(ty, &imap, Some(self_ty)));
                }
            }
        }
        map
    }

    /// What a projection stands for once its base is concrete: the choice the
    /// implementation for that base made (spec §8.5). `None` while the base is still a
    /// parameter, when the projection is itself the answer.
    pub fn project(&self, p: &ty::Proj) -> Option<Ty> {
        if ty::is_abstract(&p.on) {
            return None;
        }
        if p.on.is_wildcard() {
            return Some(Ty::Error);
        }
        // The machine's types implement the arithmetic traits with no `impl` to choose an
        // `Output`, and what they produce is themselves: `i32 + i32` is an `i32` (§11.2).
        if let Some(trait_name) = self.prelude_names.get(&p.trait_sym) {
            if &*p.name == "Output" && self.builtin_implements(&p.on, trait_name, &p.trait_args) {
                return Some(p.on.clone());
            }
        }
        let r =
            TraitRef { trait_sym: p.trait_sym, args: p.trait_args.clone(), span: Span::new(0, 0) };
        // No implementation means a bound went unchecked somewhere, which has been
        // reported where it was; the error type keeps this from saying it again.
        let Some((idx, imap)) = self.impl_of(&r, &p.on) else { return Some(Ty::Error) };
        let chosen = self.impls[idx].assoc_types.iter().find(|a| *a.name == *p.name);
        let Some(chosen) = chosen.and_then(|a| a.ty.as_ref()) else { return Some(Ty::Error) };
        Some(self.normalize(&ty::substitute_with(chosen, &imap, Some(&p.on))))
    }

    /// `t` with every projection whose base is now concrete replaced by what the
    /// implementation chose (spec §8.5). Substitution can make a base concrete —
    /// `T::Item` with `T = SliceIter<i32>` is `&i32` — so everything that substitutes into
    /// a type a body will be checked or emitted with goes through [`TypeTable::subst`],
    /// which ends here.
    pub fn normalize(&self, t: &Ty) -> Ty {
        if !ty::has_assoc(t) {
            return t.clone();
        }
        match t {
            Ty::Assoc(p) => {
                let p = ty::Proj {
                    on: self.normalize(&p.on),
                    trait_sym: p.trait_sym,
                    trait_args: p.trait_args.iter().map(|a| self.normalize(a)).collect(),
                    name: p.name.clone(),
                };
                self.project(&p).unwrap_or_else(|| Ty::Assoc(Rc::new(p)))
            }
            _ => ty::map_parts(t, &|x| self.normalize(x)),
        }
    }

    /// [`ty::substitute`], then [`TypeTable::normalize`]: a substitution as the checker and
    /// monomorphization need it, with no projection left whose base is known.
    pub fn subst(&self, t: &Ty, map: &HashMap<u32, Ty>) -> Ty {
        self.normalize(&ty::substitute(t, map))
    }

    /// [`ty::substitute_with`], then [`TypeTable::normalize`].
    pub fn subst_with(&self, t: &Ty, map: &HashMap<u32, Ty>, self_ty: Option<&Ty>) -> Ty {
        self.normalize(&ty::substitute_with(t, map, self_ty))
    }

    /// What a projection is known to implement: the bounds its trait puts on the
    /// associated type, `type Iter : Iterate`, as seen through the projection's trait
    /// arguments, with `Self` read as the projection itself (spec §8.5).
    pub fn assoc_bounds(&self, p: &ty::Proj) -> Vec<TraitRef> {
        let decl =
            self.assoc_decls.get(&p.trait_sym).and_then(|d| d.iter().find(|a| *a.name == *p.name));
        let Some(decl) = decl else { return Vec::new() };
        let me = Ty::Assoc(Rc::new(p.clone()));
        let map: HashMap<u32, Ty> = self
            .type_params(p.trait_sym)
            .iter()
            .map(|d| d.param.id)
            .zip(p.trait_args.iter().cloned())
            .collect();
        decl.bounds
            .iter()
            .map(|b| TraitRef {
                trait_sym: b.trait_sym,
                args: b.args.iter().map(|a| ty::substitute_with(a, &map, Some(&me))).collect(),
                span: b.span,
            })
            .collect()
    }

    /// What a type nobody has chosen yet may do — a parameter's bounds, or a projection's
    /// (spec §8.1, §8.5). Empty for anything else, which has implementations instead.
    pub fn abstract_bounds(&self, t: &Ty) -> Vec<TraitRef> {
        match t {
            Ty::Param(p) => self.bounds_of(p).to_vec(),
            Ty::Assoc(p) => self.assoc_bounds(p),
            _ => Vec::new(),
        }
    }

    /// One of a trait's methods, as seen through `r` applied to `self_ty` (spec §7.6).
    ///
    /// Three substitutions happen at once: the trait's own parameters become `r`'s
    /// arguments, its associated types become the implementer's choices, and `Self` becomes
    /// the type the trait is applied to. Reading `Mul<f32>`'s `mul` off a `Vec3` that chose
    /// `type Output = Vec3` therefore gives `fn mul(&self, rhs: &f32) -> Vec3`.
    pub fn trait_method(&self, r: &TraitRef, self_ty: &Ty, name: &str) -> Option<FnSig> {
        let sig = self.struct_sig(r.trait_sym)?.methods.get(name)?;
        let mut map = r.subst_map(self.type_params(r.trait_sym));
        map.extend(self.assoc_bindings(r, self_ty));
        Some(FnSig {
            params: sig
                .params
                .iter()
                .map(|p| ParamSig {
                    name: p.name.clone(),
                    ty: self.subst_with(&p.ty, &map, Some(self_ty)),
                    has_default: p.has_default,
                    caller_default: p.caller_default.clone(),
                    c_cast: p.c_cast.clone(),
                })
                .collect(),
            ret: self.subst_with(&sig.ret, &map, Some(self_ty)),
            ..sig.clone()
        })
    }

    pub fn struct_sig(&self, id: SymbolId) -> Option<&StructSig> {
        match self.sigs.get(&id)? {
            ItemSig::Struct(s) | ItemSig::Trait(s) => Some(s),
            _ => None,
        }
    }

    /// Where a type's methods are: a struct's or trait's signature, or a union's methods
    /// (spec §7.5). For method lookup only — a union has no fields or constructors, and
    /// asking [`TypeTable::struct_sig`] about one rightly says so.
    pub fn methods_sig(&self, id: SymbolId) -> Option<&StructSig> {
        match self.sigs.get(&id)? {
            ItemSig::Struct(s) | ItemSig::Trait(s) => Some(s),
            ItemSig::Union { methods, .. } => Some(methods),
            _ => None,
        }
    }

    /// `sym` and then each of its bases, nearest first. Stops at a cycle, which
    /// `flatten_bases` has already reported and broken (spec §7.3, 18.32).
    pub fn with_bases(&self, sym: SymbolId) -> Vec<SymbolId> {
        let mut chain = vec![sym];
        let mut at = self.struct_sig(sym).and_then(|s| s.base);
        while let Some(b) = at {
            if chain.contains(&b) {
                break;
            }
            chain.push(b);
            at = self.struct_sig(b).and_then(|s| s.base);
        }
        chain
    }

    /// Which type in `sym`'s chain declares a method of this name — `sym` itself if it
    /// does, else the nearest base that does. A derived type's own method wins, which is
    /// what makes `override` a replacement rather than an ambiguity (spec §7.3).
    pub fn method_owner(&self, sym: SymbolId, name: &str) -> Option<SymbolId> {
        self.with_bases(sym)
            .into_iter()
            .find(|s| self.methods_sig(*s).is_some_and(|x| x.overloads.contains_key(name)))
    }

    /// `sym` at `args`, seen as its ancestor `target`: the type arguments `target` has in
    /// that chain. `Button : Panel<i32>` seen as `Panel` is `[i32]`; `Slider<f32>`, with
    /// `Slider<T> : Panel<T>`, is `[f32]`. Empty when `target` is not generic, and `args`
    /// themselves when `target` is `sym` (spec §7.3, T-0035).
    pub fn args_as(&self, sym: SymbolId, args: &[Ty], target: SymbolId) -> Vec<Ty> {
        let mut at = sym;
        let mut at_args = args.to_vec();
        for _ in 0..self.with_bases(sym).len() {
            if at == target {
                return at_args;
            }
            let Some(s) = self.struct_sig(at) else { break };
            let Some(base) = s.base else { break };
            let map: HashMap<u32, Ty> = self
                .type_params(at)
                .iter()
                .map(|p| p.param.id)
                .zip(at_args.iter().cloned())
                .collect();
            at_args = s.base_args.iter().map(|t| self.subst(t, &map)).collect();
            at = base;
        }
        if at == target {
            at_args
        } else {
            Vec::new()
        }
    }

    /// Does `sym` inherit from `base`, at any depth? A `&Derived` is usable as a `&Base`
    /// exactly when this is true (spec §7.3, 18.32).
    pub fn inherits_from(&self, sym: SymbolId, base: SymbolId) -> bool {
        sym != base && self.with_bases(sym).contains(&base)
    }

    /// Does anything `sym` inherits from declare a `drop`, or is otherwise move-only in
    /// its own right? The derived type inherits that: C++ runs the base's destructor and
    /// has already deleted its copy operations (spec §6.3, §7.3, 18.32).
    pub fn base_has_drop(&self, sym: SymbolId) -> bool {
        self.with_bases(sym)
            .into_iter()
            .skip(1)
            .filter_map(|b| self.struct_sig(b))
            .any(|s| s.drop.is_some() || s.cpp_owned)
    }

    /// The first `virtual fn` with no body that `sym` has not been given a body for —
    /// which is what makes a type **abstract**: there is nothing to call, so no value of
    /// it can exist (spec §7.3, 18.32).
    ///
    /// The nearest declaration of a name wins, so a derived type that overrides a
    /// body-less `virtual` with a real one is concrete again.
    pub fn abstract_method(&self, sym: SymbolId) -> Option<(SymbolId, String)> {
        let chain = self.with_bases(sym);
        let mut seen: Vec<&str> = Vec::new();
        let mut found: Option<(SymbolId, String)> = None;
        for s in &chain {
            let Some(sig) = self.struct_sig(*s) else { continue };
            for key in &sig.method_keys {
                let name = key.split('#').next().unwrap_or(key);
                // A nearer declaration of the same name has already decided this one.
                if seen.contains(&name) {
                    continue;
                }
                seen.push(name);
                let Some(m) = sig.methods.get(key) else { continue };
                if m.dispatch == Dispatch::Virtual && !m.has_body && found.is_none() {
                    found = Some((*s, name.to_owned()));
                }
            }
        }
        found
    }

    pub fn is_generic(&self, id: SymbolId) -> bool {
        match self.sigs.get(&id) {
            Some(ItemSig::Fn(sig)) => sig.is_generic(),
            // A trait is generic in `Self` whether or not it is written with parameters:
            // its body talks about a type it does not choose.
            Some(ItemSig::Trait(_)) => true,
            _ => !self.type_params(id).is_empty(),
        }
    }

    // --- ownership, the type-level half of spec §6 -------------------------
    //
    // Flow analysis asks these constantly and so does codegen, so they live next to the
    // signatures they read rather than being recomputed in each consumer. The
    // flow-sensitive half — *which* value is moved *where* — is `dz_flow`.

    /// Does a value of this type need work when it dies (spec §6.5)?
    ///
    /// True for a type with a `drop`, and for anything holding one, because C++ will
    /// generate the destructor that runs it. `own ^T` on its own does not: the pointer is
    /// released by the `drop` its struct is required to have (DZ0412), not by itself.
    pub fn needs_drop(&self, ty: &Ty) -> bool {
        match ty {
            // Fields are read through the instantiation: `Pair<Owner>` needs a drop because
            // its `T` fields are `Owner`s, which the declaration alone cannot say.
            Ty::Named { sym: id, args } => match self.get(*id) {
                Some(ItemSig::Struct(s)) => {
                    s.drop.is_some()
                        || self.field_types(*id, args, &s.fields).iter().any(|t| self.needs_drop(t))
                }
                Some(ItemSig::Union { variants, .. }) => variants
                    .iter()
                    .any(|v| self.payload_types(*id, args, v).iter().any(|t| self.needs_drop(t))),
                _ => false,
            },
            // Inside a generic body `T` may be anything, so it is assumed to need dropping.
            // That is the conservative answer, and the one that keeps flow analysis honest
            // about a body checked once for every `T` it will ever be given (spec §8.2).
            Ty::Param(_) | Ty::Assoc(_) => true,
            // A fixed array's elements die with it. A slice is a view and owns nothing.
            Ty::Array { elem, .. } => self.needs_drop(elem),
            Ty::Tuple(parts) => parts.iter().any(|t| self.needs_drop(t)),
            _ => false,
        }
    }

    /// Spec §6.3. A type is copyable if it declares a `copy`, or if it declares neither
    /// `copy` nor `drop`, has no `own` field, and every field is copyable.
    ///
    /// Everything not named here is copyable: primitives, pointers, references and slices
    /// are all bit-copies, and a `^T` copies the pointer, not the pointee — `own ^T` is
    /// the spelling that says otherwise.
    pub fn copyable(&self, ty: &Ty) -> bool {
        self.copyable_guarded(ty, &mut Vec::new())
    }

    /// Is this an imported handle — a C struct whose header never defines it (spec §15.2)?
    pub fn is_opaque(&self, ty: &Ty) -> bool {
        matches!(ty, Ty::Named { sym, .. }
            if matches!(self.get(*sym), Some(ItemSig::Struct(s)) if s.opaque))
    }

    /// Is `p` bounded by `Copy`, directly or through a supertrait (spec §8.2)?
    pub fn param_is_copy(&self, p: &ty::Param) -> bool {
        self.abstract_is_copy(&Ty::Param(p.clone()))
    }

    /// Is a type nobody has chosen yet bounded by `Copy` — a parameter by its bounds, a
    /// projection by the ones its trait puts on it (spec §8.2, §8.5)?
    pub fn abstract_is_copy(&self, t: &Ty) -> bool {
        let Some(copy) = self.copy_trait else { return false };
        self.trait_closure(&self.abstract_bounds(t), t).iter().any(|r| r.trait_sym == copy)
    }

    /// The prelude's `Copy` trait, if the prelude declares it.
    pub fn copy_trait(&self) -> Option<SymbolId> {
        self.copy_trait
    }

    fn copyable_guarded(&self, ty: &Ty, stack: &mut Vec<SymbolId>) -> bool {
        match ty {
            // An `own ^T` *value* is one pointer, and reading it is a bit-copy like any
            // other. What `own` makes move-only is the **struct that declares it**, which
            // is the rule immediately below — spec §6.3 is careful to say "`own` on a
            // field". Treating the pointer itself as move-only would make `if handle !=
            // null` inside the type's own `drop` a diagnostic, which is absurd.
            Ty::Array { elem, .. } => self.copyable_guarded(elem, stack),
            // A `T?` holds a `T`, so it copies exactly when that does. It used to fall to the
            // catch-all below, which let `or` take a move-only `T` out of a `&T?`.
            Ty::Optional(inner) => self.copyable_guarded(inner, stack),
            Ty::Tuple(parts) => parts.iter().all(|t| self.copyable_guarded(t, stack)),
            Ty::Named { sym: id, args } => {
                // A struct cannot contain itself by value, so a cycle here means the
                // program is already ill-formed; stop rather than recurse forever.
                if stack.contains(id) {
                    return true;
                }
                stack.push(*id);
                let answer = match self.get(*id) {
                    Some(ItemSig::Struct(s)) => {
                        // A handle has no size here, so there is nothing to copy.
                        if s.opaque {
                            false
                        } else if s.copy.is_some() {
                            // An `impl Copy` makes the type copyable wherever it applies:
                            // `impl<T: Copy> Copy for Buffer<T>` covers `Buffer<f32>` and
                            // not `Buffer<File>`, which is move-only (spec §6.1, §8.2).
                            match self.copy_impls.get(id) {
                                Some(&i) => self.impl_applies(&self.impls[i], ty).is_some(),
                                None => true,
                            }
                        } else if s.drop.is_some() || s.cpp_owned {
                            false
                        } else if self.base_has_drop(*id) {
                            // A base's `drop` is the derived type's too: C++ runs it as
                            // part of `~Derived()`, and it deleted the base's copy
                            // operations, so a `Derived` is move-only whether or not it
                            // declares anything itself (spec §6.3, §7.3, 18.32). Without
                            // this the checker would allow a copy that C++ refuses, which
                            // is a compiler bug by invariant 7.
                            false
                        } else {
                            // "no `own` field, and every field is copyable" (spec §6.3),
                            // read through the instantiation so `Pair<Owner>` is move-only.
                            self.field_types(*id, args, &s.fields).iter().all(|t| {
                                !matches!(t, Ty::OwnPtr { .. }) && self.copyable_guarded(t, stack)
                            })
                        }
                    }
                    // A union is copyable only when every variant is (spec §7.5, 18.52).
                    Some(ItemSig::Union { variants, .. }) => variants.iter().all(|v| {
                        self.payload_types(*id, args, v)
                            .iter()
                            .all(|t| self.copyable_guarded(t, stack))
                    }),
                    _ => true,
                };
                stack.pop();
                answer
            }
            // An unbounded `T` may be *moved* and nothing more (spec §8.2). Treating it as
            // copyable would let `fn dup<T>(a: T) -> (T, T) { return (a, a) }` through, and
            // the instantiation for a move-only type would then duplicate a resource. A
            // `T: Copy` is what says otherwise.
            Ty::Param(p) => self.param_is_copy(p),
            // `T::Item` likewise, by what its trait says of it: `Iterate`'s `Item` may be
            // anything, so it is moved, never copied (spec §8.5).
            Ty::Assoc(_) => self.abstract_is_copy(ty),
            _ => true,
        }
    }

    /// A declaration's field types as seen from one instantiation of it (spec §8.4).
    pub fn field_types(&self, sym: SymbolId, args: &[Ty], fields: &[FieldSig]) -> Vec<Ty> {
        let map: HashMap<u32, Ty> =
            self.type_params(sym).iter().map(|p| p.param.id).zip(args.iter().cloned()).collect();
        fields.iter().map(|f| self.subst(&f.ty, &map)).collect()
    }

    /// What a union variant holds, as types: its fields as the instance `args` sees them,
    /// or the struct a bare variant carries (spec §7.5, 18.45). A carried struct has no
    /// field list of its own on the variant, so reading `fields` alone missed it — and with
    /// it any `drop` it has.
    pub fn payload_types(&self, sym: SymbolId, args: &[Ty], v: &VariantSig) -> Vec<Ty> {
        let mut types = self.field_types(sym, args, &v.fields);
        types.extend(v.carries.map(Ty::plain));
        types
    }

    /// Whether a union needs special members that switch on its tag: some variant has a
    /// payload C++ cannot copy, build or destroy without running code (spec §7.5, 18.52).
    pub fn union_is_trivial(&self, sym: SymbolId, args: &[Ty]) -> bool {
        let Some(ItemSig::Union { variants, .. }) = self.get(sym) else { return true };
        variants.iter().all(|v| {
            let special = v.carries.and_then(|c| self.struct_sig(c)).is_some_and(|s| {
                !s.ctors.is_empty() || s.copy.is_some() || s.base.is_some() || s.polymorphic
            });
            !special && self.payload_types(sym, args, v).iter().all(|t| self.trivially_copyable(t))
        })
    }

    /// Spec §5.5's `is_trivially_copyable`: no `drop`, no `copy`, and every field trivially
    /// copyable — a type `memcpy` may duplicate. Stricter than [`TypeTable::copyable`],
    /// which a user-written `copy` satisfies.
    pub fn trivially_copyable(&self, ty: &Ty) -> bool {
        match ty {
            Ty::Named { sym, args } => match self.get(*sym) {
                Some(ItemSig::Struct(s)) => {
                    // A C++ class with a virtual function is **never** trivially
                    // copyable — the standard requires none, and it carries a vtable
                    // pointer that a `memcpy` between two objects would corrupt. So a
                    // polymorphic Dazzle struct must answer `false` here, or `Buffer<T>`
                    // would take its `memcpy` path over one (spec §5.5, §7.3, 18.32).
                    !s.opaque
                        && !s.polymorphic
                        && s.drop.is_none()
                        && s.copy.is_none()
                        && !self.base_has_drop(*sym)
                        && self
                            .field_types(*sym, args, &s.fields)
                            .iter()
                            .all(|t| !matches!(t, Ty::OwnPtr { .. }) && self.trivially_copyable(t))
                }
                Some(ItemSig::Union { variants, .. }) => variants.iter().all(|v| {
                    self.payload_types(*sym, args, v).iter().all(|t| self.trivially_copyable(t))
                }),
                _ => true,
            },
            Ty::Array { elem, .. } => self.trivially_copyable(elem),
            Ty::Tuple(parts) => parts.iter().all(|t| self.trivially_copyable(t)),
            // Unknown until monomorphization, where the query is answered for real.
            Ty::Param(_) | Ty::Assoc(_) => false,
            _ => true,
        }
    }

    /// Why `ty` is move-only, phrased for the `help:` of a diagnostic. `None` when it is
    /// copyable after all.
    pub fn move_only_because(&self, ty: &Ty) -> Option<String> {
        if self.copyable(ty) {
            return None;
        }
        Some(match ty {
            Ty::Param(_) => {
                "a generic parameter can only be copied when it is bounded by `Copy` (spec \
                 §8.2)"
                    .to_owned()
            }
            Ty::Assoc(_) => {
                "an associated type of a generic parameter can only be copied when its trait \
                 bounds it by `Copy` (spec §8.5)"
                    .to_owned()
            }
            Ty::Named { sym: id, .. } => match self.get(*id) {
                Some(ItemSig::Struct(s)) if s.copy.is_some() => {
                    "its `impl Copy` does not apply to these type arguments (spec §8.2)".to_owned()
                }
                Some(ItemSig::Struct(s)) if s.cpp_owned => {
                    "the C++ class it is imported from has a destructor and no copy \
                     constructor, so copying it would destroy the same resource twice \
                     (spec §15.2)"
                        .to_owned()
                }
                Some(ItemSig::Struct(s)) if s.drop.is_some() => {
                    "it declares a `drop`, so copying it would release the same resource \
                     twice"
                        .to_owned()
                }
                Some(ItemSig::Struct(s)) => {
                    match s.fields.iter().find(|f| matches!(f.ty, Ty::OwnPtr { .. })) {
                        Some(f) => format!(
                            "its field `{}` is `own`, so the struct is responsible for what it points at",
                            f.name
                        ),
                        None => {
                            let field = s
                                .fields
                                .iter()
                                .find(|f| !self.copyable(&f.ty))
                                .map(|f| f.name.clone())
                                .unwrap_or_default();
                            format!("its field `{field}` is move-only")
                        }
                    }
                }
                _ => "it owns a resource".to_owned(),
            },
            _ => "it owns a resource".to_owned(),
        })
    }
}

pub struct CheckResult {
    pub table: TypeTable,
    /// Every checked body, resolved and typed (spec §17.1). Flow analysis (M5) and
    /// codegen consume this rather than the AST.
    pub hir: Hir,
    pub diagnostics: Diagnostics,
}

/// Check every module in the workspace.
///
/// `map` is here so a diagnostic can carry the **edit** that resolves it and not only a
/// sentence describing one (spec §17.4): the missing arms of a `switch` go in at a
/// particular offset, indented to match the code around them, and only the source text
/// says where that is. Nothing else in checking reads it.
pub fn check(
    ws: &Workspace,
    res: &Resolution,
    files: &HashMap<ModuleId, Vec<ParsedFile>>,
    map: &SourceMap,
) -> CheckResult {
    check_from(ws, res, files, map, None)
}

/// The checker's state once the prelude's layer — the standard library and the headers it
/// imports — is checked, and nothing else yet.
///
/// That layer sees nothing past itself, and `dz_project` gives its modules, files and so
/// its symbols the first ids in every workspace, so this state is the same for every
/// project the standard library is loaded into. Computed once, it stands in for checking
/// the standard library again in each analysis — most of what checking a small program
/// costs, paid by the editor on every pause in typing (spec §17.2).
#[derive(Clone)]
pub struct PreludeLayer {
    table: TypeTable,
    hir: Hir,
    diags: Diagnostics,
    next_param: u32,
    consts: HashMap<SymbolId, (ConstDecl, ModuleId, FileId)>,
    impl_params: HashMap<(FileId, u32), Vec<ParamDef>>,
    impl_at: HashMap<(FileId, u32), usize>,
    /// How many symbols the layer's resolution had: the cheap check that a layer is being
    /// reused against the resolution it was made from.
    symbols: usize,
}

/// Check the prelude's layer alone, for [`check_from`] to start from.
pub fn check_prelude(
    ws: &Workspace,
    res: &Resolution,
    files: &HashMap<ModuleId, Vec<ParsedFile>>,
    map: &SourceMap,
) -> PreludeLayer {
    let mut cx = Ctx::new(ws, res, map);
    cx.check_layer(files, true);
    PreludeLayer {
        table: cx.table,
        hir: cx.hir,
        diags: cx.diags,
        next_param: cx.next_param,
        consts: cx.consts,
        impl_params: cx.impl_params,
        impl_at: cx.impl_at,
        symbols: prelude_symbols(ws, res),
    }
}

/// [`check`], starting from the prelude's layer already checked when `prelude` is given.
///
/// The caller vouches that `prelude` was made from the same standard library: the same
/// `#if` flags and the same imported headers. Every other input is the workspace's own and
/// cannot reach into the layer. A layer whose symbol count does not match is not used.
pub fn check_from(
    ws: &Workspace,
    res: &Resolution,
    files: &HashMap<ModuleId, Vec<ParsedFile>>,
    map: &SourceMap,
    prelude: Option<&PreludeLayer>,
) -> CheckResult {
    let mut cx = Ctx::new(ws, res, map);
    match prelude.filter(|p| p.symbols == prelude_symbols(ws, res)) {
        Some(p) => {
            let p = p.clone();
            cx.table = p.table;
            cx.hir = p.hir;
            cx.diags = p.diags;
            cx.next_param = p.next_param;
            cx.consts = p.consts;
            cx.impl_params = p.impl_params;
            cx.impl_at = p.impl_at;
        }
        None => cx.check_layer(files, true),
    }
    cx.check_layer(files, false);
    cx.diags.sort_by_position();
    CheckResult { table: cx.table, hir: cx.hir, diagnostics: cx.diags }
}

/// The prelude's layer's symbols, which resolution numbers first.
fn prelude_symbols(ws: &Workspace, res: &Resolution) -> usize {
    res.symbols.iter().take_while(|s| ws.in_prelude_layer(s.module)).count()
}

impl<'a> Ctx<'a> {
    fn new(ws: &'a Workspace, res: &'a Resolution, map: &'a SourceMap) -> Ctx<'a> {
        let mut cx = Ctx {
            ws,
            res,
            map,
            diags: Diagnostics::new(),
            table: TypeTable {
                tests: Vec::new(),
                benches: Vec::new(),
                sigs: HashMap::new(),
                type_params: HashMap::new(),
                param_bounds: HashMap::new(),
                assoc_params: HashMap::new(),
                assoc_decls: HashMap::new(),
                declared_traits: HashMap::new(),
                copy_trait: None,
                drop_trait: None,
                impls: Vec::new(),
                drop_impls: HashMap::new(),
                copy_impls: HashMap::new(),
                prelude_names: HashMap::new(),
            },
            hir: Hir::default(),
            module: ModuleId(0),
            file: FileId(0),
            param_scopes: Vec::new(),
            next_param: 0,
            consts: HashMap::new(),
            impl_params: HashMap::new(),
            impl_at: HashMap::new(),
        };
        cx.table.copy_trait = cx.prelude_trait("Copy");
        cx.table.drop_trait = cx.prelude_trait("Drop");
        // The standard library's traits, `Float` and `Integer` among them, by name (spec §14.4).
        for (i, s) in res.symbols.iter().enumerate() {
            if s.module == res.prelude && s.kind == SymbolKind::Trait {
                cx.table.prelude_names.insert(SymbolId(i as u32), s.name.clone());
            }
        }
        cx
    }

    /// Check one layer of the workspace: the prelude's when `prelude` is true, which needs
    /// nothing from outside it, and otherwise everything else, which needs it done first.
    /// Each pass below runs over a whole layer before the next starts.
    fn check_layer(&mut self, files: &HashMap<ModuleId, Vec<ParsedFile>>, prelude: bool) {
        let ws = self.ws;
        let in_layer = |m: ModuleId| ws.in_prelude_layer(m) == prelude;

        // Imported headers first: their signatures come from libclang, and a Dazzle signature
        // may name one of their types (spec §15.2).
        self.collect_foreign(&in_layer);

        // Four passes, in strict dependency order. Each exists because the one before it
        // supplies something the next cannot do without, whatever order declarations appear in
        // — which is the point, since order never matters inside a module (spec §3.2).
        //
        //   0. Generic parameters and associated types: what names *are* types.
        //   1. Bounds and trait lists: `Mul<f32>` needs `Mul`'s parameter list to check its
        //      argument count, and the trait may be declared after the struct using it.
        //   2. Signatures: a field of type `Buffer<f32>` needs both of the above.
        //   3. Bodies.
        let run = |cx: &mut Ctx, f: &dyn Fn(&mut Ctx, &Item)| {
            for module in ws.modules().filter(|m| in_layer(m.id)) {
                cx.module = module.id;
                if let Some(module_files) = files.get(&module.id) {
                    for pf in module_files {
                        cx.file = pf.file;
                        for item in &pf.ast.items {
                            f(cx, item);
                        }
                    }
                }
            }
        };
        let impls_before = self.table.impls.len();
        run(self, &|cx, it| cx.collect_type_params(it));
        run(self, &|cx, it| cx.collect_trait_refs(it));
        run(self, &|cx, it| cx.collect_item(it));
        // `: Base` needs every struct's signature to exist before any base can be resolved,
        // and then the bases flattened parent-first — so it is its own pass, after 2 and
        // before any body is checked (spec §7.3, 18.32).
        run(self, &|cx, it| cx.collect_base(it));
        self.flatten_bases(&in_layer);
        // `impl Drop` and `impl Copy` fill in their struct's special members, which every
        // later question about ownership reads — so after every signature exists, and before
        // any body is checked (spec §6.1).
        self.attach_special_impls(impls_before);
        run(self, &check::item);
    }
}

impl Ctx<'_> {
    /// Signatures for every imported header's declarations (spec §15.2).
    ///
    /// A C struct's fields all have a default, zero, as `{}` gives them in C++ — so a
    /// literal names only the fields it sets. An unscoped enumerator or an integer macro is
    /// an untyped integer, as a literal is, since C code passes them where an `int` or an
    /// `unsigned` is wanted; a float macro written with `f` is an `f32`. Calling a C
    /// function is safe code: what it does with a pointer is the header's promise, and
    /// §15.1 calls raylib without `unsafe`.
    fn collect_foreign(&mut self, in_layer: &dyn Fn(ModuleId) -> bool) {
        use dz_project::cppimport::{ConstValue, DeclKind};
        let ws = self.ws;
        for module in ws.modules().filter(|m| in_layer(m.id)) {
            let Some(f) = &module.foreign else { continue };
            for d in &f.header.decls {
                let Some(sym) = self.res.foreign(module.id, &d.cpp) else { continue };
                let span =
                    |l: dz_project::cppimport::Loc| Span::new(l.start as usize, l.end as usize);
                let sig = match &d.kind {
                    DeclKind::Fn { params, ret, ret_cast } => ItemSig::Fn(FnSig {
                        ret_cast: *ret_cast,
                        // A C++ class's own `virtual` is its business: Dazzle calls it
                        // through C++'s dispatch and never declares one (spec §15.2).
                        dispatch: Dispatch::Static,
                        variadic: None,
                        params: params
                            .iter()
                            .map(|p| ParamSig {
                                name: p.name.clone(),
                                ty: self.foreign_ty(module.id, &p.ty),
                                has_default: false,
                                caller_default: None,
                                c_cast: p.c_cast.clone(),
                            })
                            .collect(),
                        ret: self.foreign_ty(module.id, ret),
                        fallible: false,
                        takes_self: None,
                        is_mut: false,
                        is_unsafe: false,
                        vis: Vis::Public,
                        span: span(d.loc),
                        generics: Vec::new(),
                        has_body: true,
                    }),
                    DeclKind::Struct { fields, opaque, is_union, methods, move_only } => {
                        let mut sig = StructSig {
                            fields: fields
                                .iter()
                                .map(|x| FieldSig {
                                    name: x.name.clone(),
                                    ty: self.foreign_ty(module.id, &x.ty),
                                    vis: Vis::Public,
                                    has_default: true,
                                    span: span(x.loc),
                                    bitfield: x.bitfield,
                                    declared_in: None,
                                })
                                .collect(),
                            opaque: *opaque,
                            is_union: *is_union,
                            cpp_owned: *move_only,
                            ..StructSig::default()
                        };
                        self.foreign_methods(module.id, methods, &mut sig);
                        ItemSig::Struct(sig)
                    }
                    DeclKind::Enum { backing, variants, .. } => ItemSig::Enum {
                        backing: foreign_int(*backing),
                        variants: variants
                            .iter()
                            .map(|v| VariantSig {
                                name: v.name.clone(),
                                fields: Vec::new(),
                                span: span(v.loc),
                                carries: None,
                            })
                            .collect(),
                        values: variants.iter().map(|v| Some(v.value)).collect(),
                    },
                    DeclKind::Const { value, .. } => ItemSig::Const(match value {
                        ConstValue::Int(_) => Ty::IntLit,
                        ConstValue::Float { single: true, .. } => Ty::Float(FloatTy::F32),
                        ConstValue::Float { .. } => Ty::FloatLit,
                        // A macro whose expansion is not a number but has a type of its
                        // own: raylib's `RAYWHITE` is a `Color` (spec §15.2).
                        ConstValue::Typed(t) => self.foreign_ty(module.id, t),
                    }),
                    // The alias's name already leads to its target's symbol.
                    DeclKind::Alias { .. } => continue,
                };
                self.table.sigs.insert(sym, sig);
            }
        }
    }

    /// A C++ class's member functions as Dazzle methods and constructors (spec §15.2).
    ///
    /// They land in exactly the places a Dazzle `impl` block fills, so overload resolution,
    /// visibility and receiver-mutability all work on them without a second mechanism —
    /// C++ overloading by argument types is what §18.27's method keys already express.
    fn foreign_methods(
        &mut self,
        module: ModuleId,
        methods: &[dz_project::cppimport::Method],
        sig: &mut StructSig,
    ) {
        use dz_project::cppimport::Receiver;
        for m in methods {
            let span = Span::new(m.loc.start as usize, m.loc.end as usize);
            let f = FnSig {
                dispatch: Dispatch::Static,
                variadic: None,
                params: m
                    .params
                    .iter()
                    .map(|p| ParamSig {
                        name: p.name.clone(),
                        ty: self.foreign_ty(module, &p.ty),
                        has_default: false,
                        caller_default: None,
                        c_cast: p.c_cast.clone(),
                    })
                    .collect(),
                ret: self.foreign_ty(module, &m.ret),
                ret_cast: m.ret_cast,
                fallible: false,
                takes_self: m.receiver.map(|r| matches!(r, Receiver::Ref | Receiver::MutRef)),
                is_mut: matches!(m.receiver, Some(Receiver::MutRef)),
                is_unsafe: false,
                vis: Vis::Public,
                span,
                generics: Vec::new(),
                has_body: true,
            };
            if m.is_ctor {
                sig.ctors.push(f);
                continue;
            }
            // A second method of the same name is an overload, keyed the way §18.27 keys
            // one, and resolved at the call site by its argument types (spec §15.3).
            let existing = sig.overloads.entry(m.name.clone()).or_default();
            let key = match existing.len() {
                0 => m.name.clone(),
                n => format!("{}#{n}", m.name),
            };
            existing.push(key.clone());
            sig.method_keys.push(key.clone());
            sig.methods.insert(key, f);
        }
    }

    /// A C type as Dazzle names it (spec §15.2).
    fn foreign_ty(&self, module: ModuleId, t: &dz_project::cppimport::CType) -> Ty {
        use dz_project::cppimport::CType;
        match t {
            CType::Void => Ty::Void,
            CType::Bool => Ty::Bool,
            CType::Int(i) => Ty::Int(foreign_int(*i)),
            CType::Size { signed: true } => Ty::Int(IntTy::Isize),
            CType::Size { signed: false } => Ty::Int(IntTy::Usize),
            CType::Float { bits: 32 } => Ty::Float(FloatTy::F32),
            CType::Float { .. } => Ty::Float(FloatTy::F64),
            CType::CStr => Ty::CStr,
            CType::Ptr { mutable, pointee } => {
                Ty::Ptr { mutable: *mutable, inner: Rc::new(self.foreign_ty(module, pointee)) }
            }
            CType::Ref { mutable, referent } => {
                Ty::Ref { mutable: *mutable, inner: Rc::new(self.foreign_ty(module, referent)) }
            }
            CType::Array { len, elem } => {
                Ty::Array { len: *len, elem: Rc::new(self.foreign_ty(module, elem)) }
            }
            CType::Fn { params, ret } => Ty::Fn {
                params: params.iter().map(|p| self.foreign_ty(module, p)).collect(),
                ret: Rc::new(self.foreign_ty(module, ret)),
            },
            CType::Named(n) => self.res.foreign(module, n).map(Ty::plain).unwrap_or(Ty::Error),
            // The importer leaves out whatever uses one, so this is never reached.
            CType::Unsupported(_) => Ty::Error,
        }
    }
}

fn foreign_int(i: dz_project::cppimport::CInt) -> IntTy {
    match (i.bits, i.signed) {
        (8, true) => IntTy::I8,
        (16, true) => IntTy::I16,
        (32, true) => IntTy::I32,
        (8, false) => IntTy::U8,
        (16, false) => IntTy::U16,
        (32, false) => IntTy::U32,
        (_, false) => IntTy::U64,
        (_, true) => IntTy::I64,
    }
}

pub(crate) struct Ctx<'a> {
    pub(crate) ws: &'a Workspace,
    pub(crate) res: &'a Resolution,
    /// The source, for building fixes only — see [`check`].
    pub(crate) map: &'a SourceMap,
    pub(crate) diags: Diagnostics,
    pub(crate) table: TypeTable,
    pub(crate) hir: Hir,
    pub(crate) module: ModuleId,
    pub(crate) file: FileId,
    /// Generic parameters in scope while a type is being lowered, innermost scope last
    /// (spec §8.1). A struct's parameters are in scope for its members, and a generic
    /// method adds its own on top.
    ///
    /// Signature collection and body checking both push the **same** [`ParamDef`]s, read
    /// back from the table rather than allocated twice — otherwise a field's type and the
    /// same type written in a method body would be two different `Ty::Param`s.
    pub(crate) param_scopes: Vec<HashMap<String, Ty>>,
    next_param: u32,
    /// Every module-level `const`: its declaration and where it was written. Kept so array
    /// lengths and un-annotated constants can be folded before any body is checked, across
    /// modules and in any declaration order (spec §5.3, §8.3).
    consts: HashMap<SymbolId, (ConstDecl, ModuleId, FileId)>,
    /// An `impl`'s parameters from pass 0, until pass 1 builds its [`ImplSig`]. An
    /// implementation has no symbol, so it is keyed by where it is written.
    pub(crate) impl_params: HashMap<(FileId, u32), Vec<ParamDef>>,
    /// Where each `impl` landed in [`TypeTable::impls`], by the same key.
    pub(crate) impl_at: HashMap<(FileId, u32), usize>,
}

impl<'a> Ctx<'a> {
    pub(crate) fn error(
        &mut self,
        code: &'static str,
        span: Span,
        msg: impl Into<String>,
        label: impl Into<String>,
    ) {
        self.diags.push(Diagnostic::error(code, msg.into()).at(self.file, span, label.into()));
    }

    pub(crate) fn error_help(
        &mut self,
        code: &'static str,
        span: Span,
        msg: impl Into<String>,
        label: impl Into<String>,
        help: impl Into<String>,
    ) {
        self.diags.push(
            Diagnostic::error(code, msg.into())
                .at(self.file, span, label.into())
                .with_help(help.into()),
        );
    }

    /// Render a type for a diagnostic.
    pub(crate) fn name_of(&self, t: &Ty) -> String {
        type_name(self.res, t)
    }

    /// An insertion just inside the closing brace of the construct `span` covers, with
    /// every line of `lines` indented one step past whatever line the construct opened on.
    ///
    /// All the layout knowledge a fix needs is here, so no caller reproduces it: read the
    /// opening line's own indent from the source, add four spaces, and put the text on its
    /// own lines before the brace. Returns `None` rather than guess when the span does not
    /// end in a `}` — a `switch` recovered from a syntax error, say — because an edit that
    /// is only probably right is worse than no fix at all.
    pub(crate) fn insert_inside_braces(&self, span: Span, lines: &[String]) -> Option<Edit> {
        let text = self.map.try_get(self.file)?.text();
        let (start, end) = (span.start as usize, span.end as usize);
        if end > text.len() || start >= end || !text[..end].ends_with('}') {
            return None;
        }
        let brace = end - 1;
        // The indent of the line the construct starts on: `switch` may itself be nested.
        let line_start = text[..start].rfind('\n').map_or(0, |n| n + 1);
        let outer: String =
            text[line_start..start].chars().take_while(|c| *c == ' ' || *c == '\t').collect();
        let inner = format!("{outer}    ");

        let mut body = String::new();
        for line in lines {
            body.push_str(&inner);
            body.push_str(line);
            body.push('\n');
        }
        // Where the closing brace already sits on a line of its own, that line's indent is
        // *replaced* rather than inserted before — inserting would leave a line of trailing
        // spaces above the arms. Otherwise (`switch x {}` all on one line) the break before
        // the brace is ours to add.
        let line_of_brace = text[..brace].rfind('\n').map_or(0, |n| n + 1);
        let alone = text[line_of_brace..brace].chars().all(|c| c == ' ' || c == '\t');
        let from = if alone { line_of_brace } else { brace };
        let mut out = String::new();
        if !alone {
            out.push('\n');
        }
        out.push_str(&body);
        out.push_str(&outer);
        Some(Edit { file: self.file, span: Span::new(from, brace), text: out })
    }
}

/// Symbol and type names for [`dz_hir::dump`], which stores ids and no text of its own.
pub struct Names<'a> {
    pub res: &'a Resolution,
}

impl dz_hir::dump::Names for Names<'_> {
    fn name_of_symbol(&self, id: SymbolId) -> String {
        self.res.symbol(id).name.clone()
    }

    fn name_of_type(&self, ty: &Ty) -> String {
        type_name(self.res, ty)
    }
}

/// Render a type as it is written in source.
pub fn type_name(res: &Resolution, t: &Ty) -> String {
    type_name_at(res, None, t)
}

/// A type as it would be **written in `file`** (spec §3.3).
///
/// The plain [`type_name`] gives a declaration's own name, which is what a message about
/// the declaration wants. An editor wants something else: the spelling you would have to
/// type at that position. An imported header's type is `rl::Vector2` in the file that
/// wrote `import cpp "raylib.h" as rl`, and there is no other way to name it — showing
/// `Vector2` in a hint offers a name that would not compile.
pub fn type_name_in_file(res: &Resolution, file: FileId, t: &Ty) -> String {
    type_name_at(res, Some(file), t)
}

/// How a named type is written in `file`: bare when that name reaches it there — declared
/// in the module, imported by `{ … }`, chosen in `[names]`, or from an opened dependency —
/// and `key::Name` when it arrives through a dependency key (spec §3.2).
fn qualified_for(res: &Resolution, file: FileId, sym: SymbolId) -> String {
    let symbol = res.symbol(sym);
    let name = symbol.name.clone();
    let Some(&module) = res.file_module.get(&file) else { return name };
    if module == symbol.module || res.lookup_in_file(module, file, &name) == Some(sym) {
        return name;
    }
    match res.module_spelling(file, symbol.module) {
        Some(key) => format!("{key}::{name}"),
        None => name,
    }
}

fn type_name_at(res: &Resolution, file: Option<FileId>, t: &Ty) -> String {
    let name_of = |t: &Ty| type_name_at(res, file, t);
    match t {
        Ty::Error => "{unknown}".to_owned(),
        Ty::Never => "!".to_owned(),
        Ty::Void => "void".to_owned(),
        Ty::Bool => "bool".to_owned(),
        Ty::Char => "char".to_owned(),
        Ty::Int(i) => i.name().to_owned(),
        Ty::Float(f) => f.name().to_owned(),
        Ty::Str => "str".to_owned(),
        Ty::CStr => "cstr".to_owned(),
        Ty::IntLit => "{integer}".to_owned(),
        Ty::FloatLit => "{float}".to_owned(),
        Ty::Ptr { mutable, inner } => {
            format!("^{}{}", if *mutable { "mut " } else { "" }, name_of(inner))
        }
        Ty::OwnPtr { mutable, inner } => {
            format!("own ^{}{}", if *mutable { "mut " } else { "" }, name_of(inner))
        }
        Ty::Ref { mutable, inner } => {
            format!("&{}{}", if *mutable { "mut " } else { "" }, name_of(inner))
        }
        Ty::Optional(i) => format!("{}?", name_of(i)),
        // Not writable in source: the type of a `!->` call before its `or` (spec §9).
        Ty::Fallible(i) => format!("!-> {}", name_of(i)),
        Ty::Array { len, elem } => format!("[{len}]{}", name_of(elem)),
        Ty::Slice { mutable, elem } => {
            format!("[]{}{}", if *mutable { "mut " } else { "" }, name_of(elem))
        }
        Ty::Range(e) => format!("{}..{}", name_of(e), name_of(e)),
        Ty::Tuple(elems) => {
            format!("({})", elems.iter().map(&name_of).collect::<Vec<_>>().join(", "))
        }
        Ty::Fn { params, ret } => format!(
            "fn({}) -> {}",
            params.iter().map(&name_of).collect::<Vec<_>>().join(", "),
            name_of(ret)
        ),
        Ty::Builtin(b) => b.name().to_owned(),
        Ty::Named { sym: id, args } => {
            let name = match file {
                Some(f) => qualified_for(res, f, *id),
                None => res.symbol(*id).name.clone(),
            };
            if args.is_empty() {
                name
            } else {
                // As written in source: `Buffer<f32>`, never a mangled instantiation name.
                // Diagnostics quote user syntax (principle 4, §1).
                let args = args.iter().map(&name_of).collect::<Vec<_>>().join(", ");
                format!("{name}<{args}>")
            }
        }
        Ty::Variant(id, i, _) => {
            let owner = res.symbol(*id);
            let variant =
                owner.members.iter().filter(|m| m.sort == dz_resolve::MemberSort::Variant).nth(*i);
            match variant {
                Some(v) => format!("{}::{}", owner.name, v.name),
                None => owner.name.clone(),
            }
        }
        Ty::SelfTy => "Self".to_owned(),
        Ty::Param(p) => p.name.to_string(),
        // As written: `T::Item`, and `C::Iter::Item` for a projection of one (spec §8.5).
        Ty::Assoc(p) => format!("{}::{}", name_of(&p.on), p.name),
    }
}

impl<'a> Ctx<'a> {
    // --- generic parameters (spec §8.1) -----------------------------------

    /// Allocate the parameters of one declaration and resolve their bounds.
    ///
    /// Bounds are resolved here rather than lazily because a bound that names something
    /// which is not a trait is a mistake worth reporting once, at the declaration, instead
    /// of at every use of the parameter.
    fn declare_params(&mut self, g: &Generics) -> Vec<ParamDef> {
        let mut defs = Vec::new();
        // A duplicate name is not checked here: name resolution binds generic parameters
        // through the same path as any other binding, so `<T, T>` is already DZ0306
        // (spec §7.1). Checking it again would be two diagnostics for one mistake.
        // Bounds and defaults are *not* resolved here: they may name a trait declared later
        // in the same module, whose own parameter list has to exist first. Pass 1 fills them
        // in, once every declaration's parameters are allocated.
        for p in &g.params {
            let name = p.name.name.clone();
            let id = self.next_param;
            self.next_param += 1;
            defs.push(ParamDef {
                param: ty::Param { name: Rc::from(name.as_str()), id },
                bounds: Vec::new(),
                default: None,
                span: p.name.span,
            });
        }
        defs
    }

    /// Resolve one bound or implements-entry to the trait it names, with its arguments.
    ///
    /// `self_ty` is what the trait is being applied to, which is what an omitted argument's
    /// default means: `struct Vec3 : Mul` fills `Rhs = Self` with `Vec3` (spec §11.2).
    fn trait_bound(&mut self, path: &Path, self_ty: &Ty) -> Option<TraitRef> {
        let last = path.segments.last()?;
        let names: Vec<&str> = path.segments.iter().map(|s| s.name.name.as_str()).collect();
        let (sym, used) = self.res.path_item(self.module, self.file, &names)?;
        if used != names.len() {
            return None;
        }
        if self.res.symbol(sym).kind == SymbolKind::Trait {
            let args = self.trait_args(sym, last, self_ty);
            return Some(TraitRef { trait_sym: sym, args, span: path.span });
        }
        let name = self.res.symbol(sym).name.clone();
        let what = self.res.symbol(sym).kind.describe_a();
        self.error_help(
            codes::NOT_A_TRAIT,
            path.span,
            format!("`{name}` is {what}, not a trait"),
            "not a trait",
            "a bound names a trait, which is the whole of what the parameter may then do \
             inside the body (spec §8.1)",
        );
        None
    }

    // --- constant folding (spec §5.3, §8.3) --------------------------------

    /// Fold an integer constant expression: literals, unary `-`, the arithmetic and bitwise
    /// operators, and the names of other constants — what an array length needs. `None`
    /// for anything else, for an overflow or a division by zero, and for a cycle between
    /// constants.
    pub(crate) fn const_int(&self, e: &Expr) -> Option<i128> {
        self.fold_int(e, self.module, self.file, 0)
    }

    fn fold_int(&self, e: &Expr, module: ModuleId, file: FileId, depth: u32) -> Option<i128> {
        // Deep enough for any honest chain of constants; a cycle hits it and yields None.
        if depth > 64 {
            return None;
        }
        let fold = |x: &Expr| self.fold_int(x, module, file, depth + 1);
        match &e.kind {
            ExprKind::Int(text) => parse_int_literal(text),
            ExprKind::Unary { op: UnaryOp::Neg, operand } => fold(operand)?.checked_neg(),
            ExprKind::Binary { op, lhs, rhs } => {
                let (a, b) = (fold(lhs)?, fold(rhs)?);
                match op {
                    BinaryOp::Add => a.checked_add(b),
                    BinaryOp::Sub => a.checked_sub(b),
                    BinaryOp::Mul => a.checked_mul(b),
                    BinaryOp::Div => a.checked_div(b),
                    BinaryOp::Rem => a.checked_rem(b),
                    BinaryOp::Shl => u32::try_from(b).ok().and_then(|s| a.checked_shl(s)),
                    BinaryOp::Shr => u32::try_from(b).ok().and_then(|s| a.checked_shr(s)),
                    BinaryOp::BitAnd => Some(a & b),
                    BinaryOp::BitOr => Some(a | b),
                    BinaryOp::BitXor => Some(a ^ b),
                    _ => None,
                }
            }
            ExprKind::Path(p) => {
                let sym = self.const_symbol(p, module, file)?;
                let (decl, m, f) = self.consts.get(&sym)?;
                self.fold_int(&decl.value, *m, *f, depth + 1)
            }
            _ => None,
        }
    }

    /// The constant a path names, resolved from the file the path was written in.
    fn const_symbol(&self, p: &Path, module: ModuleId, file: FileId) -> Option<SymbolId> {
        let names: Vec<&str> = p.segments.iter().map(|s| s.name.name.as_str()).collect();
        let (sym, used) = self.res.path_item(module, file, &names)?;
        if used != names.len() {
            return None;
        }
        self.consts.contains_key(&sym).then_some(sym)
    }

    /// The type of an un-annotated constant's value (spec §2.5): what its literals and the
    /// constants it names make it. An untyped literal stays untyped here so that `4 * N`
    /// with a `u8` `N` is a `u8`, and is defaulted only at the very end.
    fn const_value_type(&mut self, e: &Expr, module: ModuleId, file: FileId, depth: u32) -> Ty {
        if depth > 64 {
            return Ty::Error;
        }
        match &e.kind {
            ExprKind::Int(text) => literal_suffix_type(text).unwrap_or(Ty::IntLit),
            ExprKind::Float(text) => literal_suffix_type(text).unwrap_or(Ty::FloatLit),
            ExprKind::Str(_) | ExprKind::RawStr(_) => Ty::Str,
            ExprKind::Char(_) => Ty::Char,
            ExprKind::Bool(_) => Ty::Bool,
            ExprKind::Unary { op: UnaryOp::Neg, operand } => {
                self.const_value_type(operand, module, file, depth + 1)
            }
            ExprKind::Unary { op: UnaryOp::Not, .. } => Ty::Bool,
            ExprKind::Binary { op, lhs, rhs } => match op {
                BinaryOp::Eq
                | BinaryOp::Ne
                | BinaryOp::Lt
                | BinaryOp::Gt
                | BinaryOp::Le
                | BinaryOp::Ge
                | BinaryOp::And
                | BinaryOp::Or => Ty::Bool,
                _ => {
                    let a = self.const_value_type(lhs, module, file, depth + 1);
                    let b = self.const_value_type(rhs, module, file, depth + 1);
                    ty::join(&a, &b).unwrap_or(Ty::Error)
                }
            },
            ExprKind::Path(p) => {
                let Some(sym) = self.const_symbol(p, module, file) else { return Ty::Error };
                let Some((decl, m, f)) = self.consts.get(&sym).cloned() else {
                    return Ty::Error;
                };
                match &decl.ty {
                    // An annotated constant is its annotation, lowered where it was written.
                    Some(t) => {
                        let saved = (self.module, self.file);
                        (self.module, self.file) = (m, f);
                        let ty = self.lower_type(t);
                        (self.module, self.file) = saved;
                        ty
                    }
                    None => self.const_value_type(&decl.value, m, f, depth + 1),
                }
            }
            _ => Ty::Error,
        }
    }

    /// A type written inside `sym`'s declaration, as seen from an instantiation of it
    /// (spec §8.4). `^T` from `Buffer<T>` becomes `^f32` when read off a `Buffer<f32>`.
    ///
    /// A mismatched argument count cannot happen — `type_args` already made the list the
    /// right length — but if it somehow did, the shorter list wins and the leftover
    /// parameters stay themselves rather than being silently paired up wrongly.
    pub(crate) fn instantiate(&self, sym: SymbolId, args: &[Ty], t: &Ty) -> Ty {
        if args.is_empty() || !t.has_params() {
            return t.clone();
        }
        let map: HashMap<u32, Ty> = self
            .table
            .type_params(sym)
            .iter()
            .map(|p| p.param.id)
            .zip(args.iter().cloned())
            .collect();
        self.table.subst(t, &map)
    }

    /// A whole signature as seen from an instantiation (spec §8.4).
    pub(crate) fn instantiate_sig(&self, sym: SymbolId, args: &[Ty], sig: &FnSig) -> FnSig {
        if args.is_empty() {
            return sig.clone();
        }
        FnSig {
            params: sig
                .params
                .iter()
                .map(|p| ParamSig {
                    name: p.name.clone(),
                    ty: self.instantiate(sym, args, &p.ty),
                    has_default: p.has_default,
                    caller_default: p.caller_default.clone(),
                    c_cast: p.c_cast.clone(),
                })
                .collect(),
            ret: self.instantiate(sym, args, &sig.ret),
            ..sig.clone()
        }
    }

    /// One of the prelude's traits, by name (spec §11.2, §18.23).
    ///
    /// Looked up rather than hard-coded as a symbol id, because the prelude is ordinary
    /// Dazzle source: if `Ord` is renamed there, this follows. Returns `None` only if the
    /// prelude failed to compile, in which case its own diagnostics already say why and the
    /// operator is left alone rather than reported twice.
    pub(crate) fn prelude_trait(&self, name: &str) -> Option<SymbolId> {
        self.prelude_decl(name, SymbolKind::Trait)
    }

    /// `Ordering`, which `Ord::cmp` returns and the four ordering operators test against.
    pub(crate) fn prelude_enum(&self, name: &str) -> Option<SymbolId> {
        self.prelude_decl(name, SymbolKind::Enum)
    }

    pub(crate) fn prelude_decl(&self, name: &str, kind: SymbolKind) -> Option<SymbolId> {
        let sym = self.res.lookup(self.res.prelude, name)?;
        (self.res.symbol(sym).kind == kind).then_some(sym)
    }

    /// What `Self` means inside `sym`'s body: the declaration applied to its own
    /// parameters, so `Self` inside `Buffer<T>` is `Buffer<T>` and not a bare `Buffer`.
    pub(crate) fn self_type_of(&self, sym: SymbolId) -> Ty {
        let args = self.table.type_params(sym).iter().map(|p| Ty::Param(p.param.clone())).collect();
        Ty::Named { sym, args }
    }

    /// The argument list for a trait reference, filling omitted trailing arguments from the
    /// trait's parameter defaults (spec §11.2).
    fn trait_args(&mut self, trait_sym: SymbolId, seg: &PathSegment, self_ty: &Ty) -> Vec<Ty> {
        let params: Vec<ParamDef> = self.table.type_params(trait_sym).to_vec();
        let written: Vec<Ty> = seg.generics.iter().map(|t| self.lower_type(t)).collect();
        if written.len() > params.len() {
            let name = self.res.symbol(trait_sym).name.clone();
            let (wanted, got) = (params.len(), written.len());
            self.error_help(
                codes::WRONG_TYPE_ARG_COUNT,
                seg.span,
                format!(
                    "`{name}` takes {wanted} type {}, but {got} {} given",
                    plural(wanted, "argument"),
                    if got == 1 { "was" } else { "were" }
                ),
                format!("expected {wanted}, found {got}"),
                format!("`{name}` is declared with {wanted} type {}", plural(wanted, "parameter")),
            );
        }
        let empty = HashMap::new();
        let mut args = Vec::with_capacity(params.len());
        for (i, p) in params.iter().enumerate() {
            match written.get(i) {
                Some(t) => args.push(t.clone()),
                None => match &p.default {
                    // `Rhs = Self` becomes the type the trait is applied to.
                    Some(d) => args.push(ty::substitute_with(d, &empty, Some(self_ty))),
                    None => {
                        let name = self.res.symbol(trait_sym).name.clone();
                        let pn = p.name();
                        self.error_help(
                            codes::WRONG_TYPE_ARG_COUNT,
                            seg.span,
                            format!("`{name}` needs a type argument for `{pn}`"),
                            format!("`{pn}` has no default"),
                            format!("write `{name}<{pn}>` (spec §8.1)"),
                        );
                        args.push(Ty::Error);
                    }
                },
            }
        }
        args
    }

    /// Bring the parameters pass 0 allocated for `sym` into scope. Always pushes a scope,
    /// even an empty one, so every `push` has exactly one `pop`.
    pub(crate) fn push_declared_params(&mut self, sym: SymbolId) {
        let defs = self.table.type_params.get(&sym).cloned().unwrap_or_default();
        self.push_params(&defs);
        // A trait's associated types are in scope for its own members, so `fn get(&self) ->
        // Item` resolves (spec §8.5). They go in the same scope as the generic parameters,
        // since neither can shadow the other — both are names the declaration introduces.
        let assoc: Vec<(String, Ty)> = self
            .table
            .assoc_params(sym)
            .iter()
            .map(|(n, p)| (n.clone(), Ty::Param(p.clone())))
            .collect();
        if let Some(scope) = self.param_scopes.last_mut() {
            scope.extend(assoc);
        }
    }

    /// Bring a declaration's parameters into scope for whatever is lowered next.
    pub(crate) fn push_params(&mut self, defs: &[ParamDef]) {
        let scope =
            defs.iter().map(|d| (d.name().to_owned(), Ty::Param(d.param.clone()))).collect();
        self.param_scopes.push(scope);
    }

    pub(crate) fn pop_params(&mut self) {
        self.param_scopes.pop();
    }

    pub(crate) fn param_in_scope(&self, name: &str) -> Option<Ty> {
        self.param_scopes.iter().rev().find_map(|s| s.get(name).cloned())
    }

    // --- resolving syntax types into `Ty` ---------------------------------

    /// Lower a written type, refusing an imported handle anywhere a value of it would have
    /// to exist: its header never defines it, so nothing here knows its size (spec §15.2,
    /// §18.40). Behind a pointer — which is how a C library hands one out — it is fine.
    pub(crate) fn lower_type(&mut self, t: &Type) -> Ty {
        let ty = self.lower_type_inner(t);
        // An abstract type has a method with no body, so a value of it would have nothing
        // to call. Behind a reference or a pointer it is exactly what `virtual` is for
        // (spec §7.3, 18.32). Checked here, as a handle's by-value use is, so C++ never
        // has to report it — a C++ error in generated code is a compiler bug (invariant 7).
        if let Some(sym) = self.abstract_by_value(&ty) {
            if let Some((owner, method)) = self.table.abstract_method(sym) {
                let name = self.res.symbol(sym).name.clone();
                let owner = self.res.symbol(owner).name.clone();
                self.error_help(
                    codes::ABSTRACT_TYPE,
                    t.span,
                    format!("`{name}` is abstract, so there is no value of it"),
                    "abstract type by value",
                    format!(
                        "`{owner}::{method}` is a `virtual fn` with no body — write \
                         `&{name}` or `^{name}` and pass a type that overrides it \
                         (spec §7.3)"
                    ),
                );
            }
        }
        if let Some(sym) = self.opaque_by_value(&ty) {
            let name = self.res.symbol(sym).name.clone();
            self.error_help(
                codes::OPAQUE_BY_VALUE,
                t.span,
                format!("`{name}` has no definition in its header"),
                "its size is unknown here",
                format!(
                    "the header only declares `{name}`, so it is a handle: write `^{name}` \
                     or `^mut {name}` and let the library make one (spec §15.2)"
                ),
            );
        }
        ty
    }

    /// The first abstract struct inside `t` that is not behind a pointer or a reference.
    /// Same shape as [`Ctx::opaque_by_value`], and for the same reason: both are types
    /// that exist only to be pointed at (spec §7.3, §15.2).
    fn abstract_by_value(&self, t: &Ty) -> Option<SymbolId> {
        match t {
            Ty::Ptr { .. } | Ty::OwnPtr { .. } | Ty::Ref { .. } => None,
            Ty::Named { sym, .. } if self.table.abstract_method(*sym).is_some() => Some(*sym),
            Ty::Array { elem, .. } | Ty::Slice { elem, .. } => self.abstract_by_value(elem),
            Ty::Optional(i) | Ty::Fallible(i) | Ty::Range(i) => self.abstract_by_value(i),
            Ty::Tuple(parts) => parts.iter().find_map(|p| self.abstract_by_value(p)),
            Ty::Named { args, .. } => args.iter().find_map(|a| self.abstract_by_value(a)),
            Ty::Fn { params, ret } => {
                params.iter().chain(std::iter::once(&**ret)).find_map(|p| self.abstract_by_value(p))
            }
            _ => None,
        }
    }

    /// The first imported handle inside `t` that is not behind a pointer, if there is one.
    fn opaque_by_value(&self, t: &Ty) -> Option<SymbolId> {
        match t {
            // Behind a pointer is where a handle belongs, and a reference is what `or`
            // makes of that pointer — no more a value of it than the pointer was.
            Ty::Ptr { .. } | Ty::OwnPtr { .. } | Ty::Ref { .. } => None,
            Ty::Named { sym, .. } if self.table.is_opaque(t) => Some(*sym),
            Ty::Array { elem, .. } | Ty::Slice { elem, .. } => self.opaque_by_value(elem),
            Ty::Optional(i) | Ty::Fallible(i) | Ty::Range(i) => self.opaque_by_value(i),
            Ty::Tuple(parts) => parts.iter().find_map(|p| self.opaque_by_value(p)),
            Ty::Named { args, .. } => args.iter().find_map(|a| self.opaque_by_value(a)),
            Ty::Fn { params, ret } => {
                params.iter().chain(std::iter::once(&**ret)).find_map(|p| self.opaque_by_value(p))
            }
            _ => None,
        }
    }

    fn lower_type_inner(&mut self, t: &Type) -> Ty {
        match &t.kind {
            TypeKind::Path(p) => self.lower_path_type(p, t.span),
            TypeKind::Ptr { mutable, inner } => {
                Ty::Ptr { mutable: *mutable, inner: Rc::new(self.lower_type_inner(inner)) }
            }
            TypeKind::OwnPtr { mutable, inner } => {
                Ty::OwnPtr { mutable: *mutable, inner: Rc::new(self.lower_type_inner(inner)) }
            }
            TypeKind::Ref { mutable, inner } => {
                Ty::Ref { mutable: *mutable, inner: Rc::new(self.lower_type_inner(inner)) }
            }
            TypeKind::Optional(i) => Ty::Optional(Rc::new(self.lower_type_inner(i))),
            TypeKind::Array { size, elem } => {
                let elem = Rc::new(self.lower_type_inner(elem));
                match self.const_int(size).and_then(|v| u64::try_from(v).ok()) {
                    Some(len) => Ty::Array { len, elem },
                    None => {
                        self.error_help(
                            codes::TYPE_MISMATCH,
                            size.span,
                            "array length must be a constant",
                            "not a non-negative constant",
                            "an array length is an integer made of literals, operators and \
                             other `const`s, folded at compile time (spec §5.3, §8.3)",
                        );
                        // Not a slice: an unusable length makes the whole type unusable,
                        // and `Ty::Error` is what stops the mistake cascading.
                        Ty::Error
                    }
                }
            }
            TypeKind::Slice { mutable, elem } => {
                Ty::Slice { mutable: *mutable, elem: Rc::new(self.lower_type_inner(elem)) }
            }
            TypeKind::Tuple(elems) => {
                Ty::Tuple(elems.iter().map(|e| self.lower_type_inner(e)).collect())
            }
            TypeKind::Fn { params, ret } => Ty::Fn {
                params: params.iter().map(|p| self.lower_type_inner(p)).collect(),
                ret: Rc::new(ret.as_ref().map(|r| self.lower_type_inner(r)).unwrap_or(Ty::Void)),
            },
            TypeKind::SelfType => Ty::SelfTy,
            TypeKind::Error => Ty::Error,
        }
    }

    pub(crate) fn lower_path_type(&mut self, p: &Path, span: Span) -> Ty {
        let Some(first) = p.segments.first() else {
            return Ty::Error;
        };
        let name = first.name.name.as_str();

        if p.segments.len() == 1 {
            // A generic parameter shadows everything: inside `struct Buffer<T>`, `T` is
            // the parameter even if a `T` is declared at module scope (spec §8.1).
            if let Some(param) = self.param_in_scope(name) {
                return param;
            }
            if let Some(builtin) = builtin_type(name) {
                return builtin;
            }
            if let Some(sym) = self.res.lookup_in_file(self.module, self.file, name) {
                return self.named_or_error(sym, first, span);
            }
            // Name resolution already reported anything genuinely missing.
            return Ty::Error;
        }

        // `T::Item`, and `C::Iter::Item` through a projection's own bounds (spec §8.5).
        if let Some(param) = self.param_in_scope(name) {
            let mut on = param;
            for seg in &p.segments[1..] {
                on = self.project_named(on, &seg.name, span);
            }
            return on;
        }

        // `module::Type`, and `header::ns::Type` for a C++ namespace, which the path walks
        // through one segment at a time (spec §15.2).
        // And `rl::shapes::Rectangle` through a public dependency (spec §3.2).
        if self.res.module_alias(self.file, name).is_some() {
            let names: Vec<&str> = p.segments.iter().map(|s| s.name.name.as_str()).collect();
            let Some((sym, used)) = self.res.path_item(self.module, self.file, &names) else {
                return Ty::Error;
            };
            // A namespace the path stopped inside is not a type.
            if used != names.len() || self.res.symbol(sym).kind == SymbolKind::Namespace {
                return Ty::Error;
            }
            return self.named_or_error(sym, &p.segments[used - 1], span);
        }
        Ty::Error
    }

    /// `on::name`: an associated type of a type nobody has chosen yet, from whichever of its
    /// bounds declares it — `T::Item` for a `T: Iterate` (spec §8.5).
    ///
    /// Supertraits count, as they do for a bound's methods. Two bounds that both declare
    /// `name` are ambiguous rather than resolved by order: a `T: Mul<f32> + Mul<Vec2>` has
    /// an `Output` for each, and nothing in `T::Output` says which.
    pub(crate) fn project_named(&mut self, on: Ty, name: &Ident, span: Span) -> Ty {
        if on.is_error() {
            return Ty::Error;
        }
        let shown = self.name_of(&on);
        let bounds = self.table.abstract_bounds(&on);
        let mut found: Vec<TraitRef> = Vec::new();
        for r in self.table.trait_closure(&bounds, &on) {
            let declares =
                self.table.assoc_params(r.trait_sym).iter().any(|(n, _)| *n == name.name);
            if declares && !found.iter().any(|x| x.trait_sym == r.trait_sym && x.args == r.args) {
                found.push(r);
            }
        }
        // In the order the bounds were written, supertraits after, so a message lists them
        // as the reader wrote them.
        let written = |r: &TraitRef| {
            bounds
                .iter()
                .position(|b| b.trait_sym == r.trait_sym && b.args == r.args)
                .unwrap_or(bounds.len())
        };
        found.sort_by_key(written);
        let n = &name.name;
        match found.len() {
            1 => {
                let r = found.remove(0);
                Ty::Assoc(Rc::new(ty::Proj {
                    on,
                    trait_sym: r.trait_sym,
                    trait_args: r.args,
                    name: Rc::from(n.as_str()),
                }))
            }
            0 => {
                let help = if bounds.is_empty() {
                    format!(
                        "`{shown}` has no bounds, so it has no associated types — bound it by \
                         a trait that declares `type {n}` (spec §8.5)"
                    )
                } else {
                    format!(
                        "an associated type comes from a bound: none of `{shown}`'s declares \
                         `type {n}` (spec §8.5)"
                    )
                };
                self.error_help(
                    codes::NO_ASSOC_TYPE,
                    span,
                    format!("`{shown}` has no associated type `{n}`"),
                    "no bound declares it",
                    help,
                );
                Ty::Error
            }
            _ => {
                let traits: Vec<String> = found
                    .iter()
                    .map(|r| self.name_of(&Ty::Named { sym: r.trait_sym, args: r.args.clone() }))
                    .collect();
                self.error_help(
                    codes::AMBIGUOUS_ASSOC_TYPE,
                    span,
                    format!("`{shown}::{n}` could be the `{n}` of `{}`", traits.join("` or `")),
                    "ambiguous associated type",
                    format!(
                        "each of those implementations chooses its own `{n}`; bound `{shown}` \
                         by only one trait that declares it (spec §8.5, §18.29)"
                    ),
                );
                Ty::Error
            }
        }
    }

    /// Lower a written type-argument list against what the declaration expects (spec §8.1).
    ///
    /// A wrong count yields the parameters' own count of `Ty::Error`s rather than the
    /// written arguments, so that everything downstream still sees a well-formed
    /// instantiation and the one mistake produces one diagnostic.
    fn type_args(&mut self, sym: SymbolId, seg: &PathSegment) -> Vec<Ty> {
        let wanted = self.table.type_params(sym).len();
        let args: Vec<Ty> = seg.generics.iter().map(|t| self.lower_type(t)).collect();
        if args.len() == wanted {
            return args;
        }
        let name = self.res.symbol(sym).name.clone();

        // A bare `Buffer` *inside* `Buffer<T>` means `Buffer<T>` — its own parameters are
        // in scope, and writing them out again would be noise. `Self` is the other
        // spelling of the same thing.
        if args.is_empty() {
            let own: Vec<ParamDef> = self.table.type_params(sym).to_vec();
            if own.iter().all(|p| self.param_in_scope(p.name()) == Some(Ty::Param(p.param.clone())))
            {
                return own.into_iter().map(|p| Ty::Param(p.param)).collect();
            }
            self.error_help(
                codes::WRONG_TYPE_ARG_COUNT,
                seg.span,
                format!("`{name}` needs type arguments"),
                format!("expected {wanted}, found none"),
                format!(
                    "write `{name}<{}>` (spec §8.1)",
                    own.iter().map(|p| p.name()).collect::<Vec<_>>().join(", ")
                ),
            );
            return vec![Ty::Error; wanted];
        }

        let got = args.len();
        self.error_help(
            codes::WRONG_TYPE_ARG_COUNT,
            seg.span,
            format!(
                "`{name}` takes {wanted} type {}, but {got} {} given",
                plural(wanted, "argument"),
                if got == 1 { "was" } else { "were" }
            ),
            format!("expected {wanted}, found {got}"),
            format!("`{name}` is declared with {wanted} type {}", plural(wanted, "parameter")),
        );
        vec![Ty::Error; wanted]
    }

    /// Lower `Name<A, B>` once the declaration it names is known.
    fn named_or_error(&mut self, sym: SymbolId, seg: &PathSegment, span: Span) -> Ty {
        match self.res.symbol(sym).kind {
            SymbolKind::Struct | SymbolKind::Union | SymbolKind::Enum | SymbolKind::Trait => {
                let args = self.type_args(sym, seg);
                Ty::Named { sym, args }
            }
            other => {
                let name = self.res.symbol(sym).name.clone();
                self.error(
                    codes::TYPE_MISMATCH,
                    span,
                    format!("`{name}` is {} and cannot be used as a type", other.describe_a()),
                    "not a type",
                );
                Ty::Error
            }
        }
    }

    // --- signature collection ---------------------------------------------

    /// Pass 0: what names are types — one declaration's generic parameters, and the
    /// associated types a trait names but does not choose (spec §8.1, §8.5).
    ///
    /// A *method*'s own parameters are not done here. They are allocated with its
    /// signature, which is safe because nothing can name them before that signature
    /// exists — unlike a type's, which any other declaration may instantiate.
    ///
    /// **A trait's associated type is a generic parameter in disguise.** Inside the trait,
    /// `Item` is a type nobody has chosen yet, exactly like `T` inside `Buffer<T>`; the only
    /// difference is who chooses it later (§8.5). Modelling it as a `Ty::Param` means the
    /// bound checking, substitution and diagnostics built for parameters all apply to it
    /// unchanged.
    fn collect_type_params(&mut self, item: &Item) {
        if let ItemKind::Impl(i) = &item.kind {
            let defs = self.declare_params(&i.generics);
            self.impl_params.insert((self.file, item.span.start), defs);
            return;
        }
        let Some(sym) = self.symbol_of(item) else { return };
        // Constants are recorded here too: an array length in any signature may name one,
        // and signatures come two passes later.
        if let ItemKind::Const(c) = &item.kind {
            self.consts.insert(sym, (c.clone(), self.module, self.file));
            return;
        }
        let generics = match &item.kind {
            ItemKind::Struct(s) => &s.generics,
            ItemKind::Trait(t) => &t.generics,
            ItemKind::Union(u) => &u.generics,
            ItemKind::Fn(f) => &f.generics,
            _ => return,
        };
        if !generics.is_empty() {
            let params = self.declare_params(generics);
            self.table.type_params.insert(sym, params);
        }
        if let ItemKind::Trait(t) = &item.kind {
            let mut assoc = Vec::new();
            for m in &t.items {
                let MemberKind::TypeAlias { name, ty, .. } = &m.kind else { continue };
                // `type Item = X` on a trait chooses it for every implementer, so it is an
                // alias rather than a parameter and needs no id.
                if ty.is_some() {
                    continue;
                }
                let id = self.next_param;
                self.next_param += 1;
                assoc.push((
                    name.name.clone(),
                    ty::Param { name: Rc::from(name.name.as_str()), id },
                ));
            }
            if !assoc.is_empty() {
                self.table.assoc_params.insert(sym, assoc);
            }
        }
    }

    /// Pass 2b: resolve `struct Button : UiElement` to the base's symbol (spec §7.3, 18.32).
    ///
    /// Only the link is made here; the base's fields are copied in afterwards by
    /// [`Ctx::flatten_bases`], which needs every link to exist first so it can work
    /// parent-first and notice a cycle instead of recursing into one.
    fn collect_base(&mut self, item: &Item) {
        let ItemKind::Struct(s) = &item.kind else { return };
        let Some(base) = &s.base else { return };
        let Some(sym) = self.symbol_of(item) else { return };

        // A base is a plain type path, so this is `lower_type`'s work — but the errors it
        // would give ("not a type") are not the ones to give here.
        self.push_declared_params(sym);
        let ty = self.lower_path_type(base, base.span);
        self.pop_params();

        // A generic base is one instance of it, `Panel<i32>` — or, in a generic derived
        // type, one in terms of its own parameters, `Panel<T>` (T-0035). `: Panel` with no
        // arguments has been reported by the lowering, as DZ0534, and comes back an error.
        let (base_sym, base_args) = match &ty {
            Ty::Error => return,
            Ty::Named { sym: b, args } => (*b, args.clone()),
            other => {
                let shown = self.name_of(other);
                self.error_help(
                    codes::BAD_BASE,
                    base.span,
                    format!("`{shown}` cannot be a base"),
                    "not a struct",
                    "`: Base` names one struct to inherit from; a trait is implemented \
                     with `impl Trait for Type { … }` instead (spec §7.3, §7.6)",
                );
                return;
            }
        };

        let kind = self.res.symbol(base_sym).kind;
        let name = self.res.symbol(base_sym).name.clone();
        if base_sym == sym {
            self.error_help(
                codes::BAD_BASE,
                base.span,
                format!("`{name}` cannot inherit from itself"),
                "its own base",
                "a struct that contained itself would have no size (spec §7.3)",
            );
            return;
        }
        if !matches!(self.table.get(base_sym), Some(ItemSig::Struct(_)))
            || kind != SymbolKind::Struct
        {
            let what = match kind {
                SymbolKind::Trait => {
                    "a trait is implemented, not inherited from: write \
                                      `impl Trait for Type { … }` (spec §7.6)"
                }
                SymbolKind::Union => "a union is a sum type; inherit from a struct (§7.5)",
                SymbolKind::Enum => "an enum is a named integer; inherit from a struct (§7.4)",
                _ => "`: Base` names one struct to inherit from (spec §7.3)",
            };
            self.error_help(
                codes::BAD_BASE,
                base.span,
                format!("`{name}` is not a struct"),
                "not a struct",
                what,
            );
            return;
        }
        // An imported C++ class as a base is the Jolt-listener case, and it needs the
        // importer to bring a base's members in and to emit the `: Base` C++ wants; it is
        // in the deferral register rather than here.
        if self.ws.module(self.res.symbol(base_sym).module).foreign.is_some() {
            self.error_help(
                codes::BAD_BASE,
                base.span,
                format!("`{name}` is imported from C++, and cannot be a base yet"),
                "an imported base",
                "inheriting an imported class is what Jolt's listeners need and is not \
                 implemented; a Dazzle struct may inherit a Dazzle struct (spec §15.2)",
            );
            return;
        }
        if let Some(ItemSig::Struct(sig)) = self.table.sigs.get_mut(&sym) {
            sig.base = Some(base_sym);
            sig.base_args = base_args;
        }
    }

    /// Pass 2c: copy each base's fields into the types that inherit them (18.32).
    ///
    /// The base's fields come first and the type's own follow, so a field index means the
    /// same thing everywhere and a `&Derived` is a `&Base` at offset zero. Walking
    /// parent-first is what lets a chain of three work from one pass: by the time a type
    /// is flattened its base already holds *its* base's fields.
    ///
    /// Only the types of the layer being checked: an earlier layer's are flattened already,
    /// and doing one again would give it its base's fields twice.
    fn flatten_bases(&mut self, in_layer: &dyn Fn(ModuleId) -> bool) {
        let res = self.res;
        let (with_base, earlier): (Vec<SymbolId>, Vec<SymbolId>) = self
            .table
            .sigs
            .iter()
            .filter_map(|(sym, sig)| match sig {
                ItemSig::Struct(s) if s.base.is_some() => Some(*sym),
                _ => None,
            })
            .partition(|s| in_layer(res.symbol(*s).module));

        // Deterministic order: the map's iteration order is not, and a cycle's diagnostic
        // must land on the same struct every run.
        let mut with_base = with_base;
        with_base.sort_by_key(|s| s.0);

        let mut done: HashSet<SymbolId> = earlier.into_iter().collect();
        for sym in with_base {
            self.flatten_one(sym, &mut done, &mut Vec::new());
        }
    }

    /// Flatten one type, its bases first. `chain` is the path taken to get here, which is
    /// how a cycle is caught and named.
    fn flatten_one(
        &mut self,
        sym: SymbolId,
        done: &mut HashSet<SymbolId>,
        chain: &mut Vec<SymbolId>,
    ) {
        if done.contains(&sym) {
            return;
        }
        let (base, base_args) = match self.table.get(sym) {
            Some(ItemSig::Struct(s)) => (s.base, s.base_args.clone()),
            _ => return,
        };
        let Some(base) = base else {
            done.insert(sym);
            return;
        };
        if chain.contains(&sym) {
            // Break the link before reporting, so nothing downstream can walk the cycle.
            if let Some(ItemSig::Struct(s)) = self.table.sigs.get_mut(&sym) {
                s.base = None;
            }
            let names: Vec<String> = chain
                .iter()
                .chain(std::iter::once(&sym))
                .map(|s| self.res.symbol(*s).name.clone())
                .collect();
            let span = self.res.symbol(sym).span;
            let file = self.res.symbol(sym).file;
            self.diags.push(
                dz_diag::Diagnostic::error(
                    codes::BAD_BASE,
                    format!("`{}` inherits from itself", names.last().cloned().unwrap_or_default()),
                )
                .at(file, span, "this base chain comes back round")
                .with_help(format!(
                    "the chain is {} — a struct cannot contain itself (spec §7.3)",
                    names.join(" : ")
                )),
            );
            done.insert(sym);
            return;
        }
        chain.push(sym);
        self.flatten_one(base, done, chain);
        chain.pop();

        // The base may have lost its link to a cycle just above.
        let Some(ItemSig::Struct(base_sig)) = self.table.get(base) else {
            done.insert(sym);
            return;
        };
        // A generic base's fields are this type's at the base's arguments: `Panel<T>`'s
        // `value: T` is `value: i32` in `Button : Panel<i32>` (T-0035).
        let map: HashMap<u32, Ty> = self
            .table
            .type_params(base)
            .iter()
            .map(|p| p.param.id)
            .zip(base_args.iter().cloned())
            .collect();
        let base_fields: Vec<FieldSig> = base_sig
            .fields
            .iter()
            .map(|f| FieldSig {
                declared_in: Some(f.declared_in.unwrap_or(base)),
                ty: ty::substitute(&f.ty, &map),
                ..f.clone()
            })
            .collect();
        let base_polymorphic = base_sig.polymorphic;

        // A field redeclared in the derived type is an error, not a hiding: two fields of
        // one name in one type would make `self.x` mean two things (spec §7.3).
        let own: Vec<(String, Span)> = match self.table.get(sym) {
            Some(ItemSig::Struct(s)) => s.fields.iter().map(|f| (f.name.clone(), f.span)).collect(),
            _ => Vec::new(),
        };
        for (name, span) in &own {
            if let Some(b) = base_fields.iter().find(|f| &f.name == name) {
                let base_name = self.res.symbol(base).name.clone();
                let decl_in = b.declared_in.unwrap_or(base);
                let file = self.res.symbol(sym).file;
                self.diags.push(
                    dz_diag::Diagnostic::error(
                        codes::SHADOWS_BASE_MEMBER,
                        format!("field `{name}` is already inherited from `{base_name}`"),
                    )
                    .at(file, *span, "redeclared here")
                    .with_label(dz_diag::Label::secondary(
                        self.res.symbol(decl_in).file,
                        b.span,
                        "inherited from here",
                    ))
                    .with_help(
                        "a name means one thing in a type, whichever declaration it came \
                         from — rename this field, or drop it and use the base's (spec §7.3)"
                            .to_owned(),
                    ),
                );
            }
        }

        if let Some(ItemSig::Struct(s)) = self.table.sigs.get_mut(&sym) {
            s.inherited = base_fields.len();
            let mut fields = base_fields;
            fields.append(&mut s.fields);
            s.fields = fields;
            s.polymorphic = s.polymorphic || base_polymorphic;
        }
        done.insert(sym);
    }

    /// Pass 1: bounds and trait lists (spec §7.6, §8.1).
    ///
    /// Separate from pass 0 because `struct Vec3 : Mul<f32>` has to check its argument
    /// count against `Mul`'s parameter list, and `Mul` may be declared afterwards.
    fn collect_trait_refs(&mut self, item: &Item) {
        if let ItemKind::Impl(i) = &item.kind {
            self.impl_header(item, i);
            return;
        }
        let Some(sym) = self.symbol_of(item) else { return };
        let traits = match &item.kind {
            ItemKind::Trait(t) => t.supertraits.as_slice(),
            _ => &[][..],
        };

        // A parameter's bounds are resolved here for the same reason, and the parameters
        // themselves already exist, so this fills them in rather than reallocating.
        let generics = match &item.kind {
            ItemKind::Struct(s) => Some(&s.generics),
            ItemKind::Trait(t) => Some(&t.generics),
            ItemKind::Union(u) => Some(&u.generics),
            ItemKind::Fn(f) => Some(&f.generics),
            _ => None,
        };
        if let Some(g) = generics {
            self.push_declared_params(sym);
            self.resolve_param_bounds(sym, g);
            self.pop_params();
        }

        // Associated types, before any signature: a method may return one and be written
        // above it, and order never matters inside a declaration (spec §8.5).
        let members = match &item.kind {
            ItemKind::Struct(s) => Some(s.members.as_slice()),
            ItemKind::Trait(t) => Some(t.items.as_slice()),
            _ => None,
        };
        if let Some(members) = members {
            self.push_declared_params(sym);
            let decls = self.collect_assoc_types(members);
            self.pop_params();
            if !decls.is_empty() {
                self.table.assoc_decls.insert(sym, decls);
            }
        }

        if !traits.is_empty() {
            self.push_declared_params(sym);
            // A supertrait obliges the eventual implementer, so it is applied to `Self`:
            // `trait Ord : Eq` means every `Ord` is also an `Eq`.
            let self_ty = Ty::SelfTy;
            let resolved: Vec<TraitRef> =
                traits.iter().filter_map(|t| self.trait_bound(t, &self_ty)).collect();
            self.pop_params();
            self.table.declared_traits.insert(sym, resolved);
        }
    }

    /// One declaration's associated types (spec §8.5).
    pub(crate) fn collect_assoc_types(&mut self, members: &[Member]) -> Vec<AssocType> {
        let mut out: Vec<AssocType> = Vec::new();
        for m in members {
            let MemberKind::TypeAlias { name, bounds, ty } = &m.kind else { continue };
            if out.iter().any(|a| a.name == name.name) {
                let n = name.name.clone();
                self.error(
                    codes::DUPLICATE_FIELD,
                    name.span,
                    format!("associated type `{n}` is declared twice"),
                    "duplicate associated type",
                );
            }
            // A bound is on whatever the implementer chooses, so `Self` in it means that
            // implementer: `type Item : Format`.
            let bounds = bounds.iter().filter_map(|b| self.trait_bound(b, &Ty::SelfTy)).collect();
            out.push(AssocType {
                name: name.name.clone(),
                ty: ty.as_ref().map(|t| self.lower_type(t)),
                bounds,
                span: name.span,
            });
        }
        out
    }

    /// Fill in the bounds and defaults of parameters pass 0 allocated.
    fn resolve_param_bounds(&mut self, sym: SymbolId, g: &Generics) {
        let mut defs = self.table.type_params.get(&sym).cloned().unwrap_or_default();
        for (def, p) in defs.iter_mut().zip(&g.params) {
            // A bound on `T` is applied to `T` itself, so `T: Mul` means `T: Mul<T>`.
            let self_ty = Ty::Param(def.param.clone());
            def.bounds = p.bounds.iter().filter_map(|b| self.trait_bound(b, &self_ty)).collect();
            def.default = p.default.as_ref().map(|t| self.lower_type(t));
            self.table.param_bounds.insert(def.param.id, def.bounds.clone());
        }
        self.table.type_params.insert(sym, defs);
    }

    /// The struct a bare union variant carries. A bare variant always names one; a variant
    /// with no payload is written `Point {}`, so a misspelt or missing struct is an error
    /// rather than a silently empty variant (spec §7.5, 18.45).
    fn carried_struct(&mut self, v: &dz_ast::Variant) -> Option<SymbolId> {
        let name = v.name.name.as_str();
        let found = if self.param_in_scope(name).is_some() || builtin_type(name).is_some() {
            None
        } else {
            self.res.lookup_in_file(self.module, self.file, name)
        };
        let help = format!(
            "a variant with no payload is written `{name} {{}}`; a bare `{name}` carries the \
             struct of that name (spec §7.5)"
        );
        let Some(sym) = found else {
            self.error_help(
                codes::BAD_VARIANT,
                v.name.span,
                format!("no struct named `{name}` for this variant to carry"),
                "not a struct in scope",
                help,
            );
            return None;
        };
        let symbol = self.res.symbol(sym);
        if symbol.kind != SymbolKind::Struct {
            let what = symbol.kind.describe();
            self.error_help(
                codes::BAD_VARIANT,
                v.name.span,
                format!("`{name}` is a {what}, so a variant cannot carry it"),
                format!("names a {what}"),
                help,
            );
            return None;
        }
        // A bare name has nowhere to write type arguments.
        if !self.table.type_params(sym).is_empty() {
            self.error_help(
                codes::BAD_VARIANT,
                v.name.span,
                format!("`{name}` is generic, so a bare variant cannot carry it"),
                "needs type arguments",
                format!(
                    "declare the payload as fields — `{name} {{ value: {name}<…> }}` (spec §7.5)"
                ),
            );
            return None;
        }
        Some(sym)
    }

    fn collect_item(&mut self, item: &Item) {
        if let ItemKind::Impl(i) = &item.kind {
            self.impl_methods(item, i);
            return;
        }
        let Some(sym) = self.symbol_of(item) else {
            return;
        };
        match &item.kind {
            // Removed by the driver before this phase (spec §12.10).
            ItemKind::CfgIf(_) => {}
            ItemKind::Struct(s) => {
                // A type's parameters are in scope for every one of its members (§8.1).
                self.push_declared_params(sym);
                let sig = self.collect_struct(sym, &s.members);
                self.table.sigs.insert(sym, ItemSig::Struct(sig));
                self.pop_params();
            }
            ItemKind::Trait(t) => {
                self.push_declared_params(sym);
                let sig = self.collect_struct(sym, &t.items);
                self.table.sigs.insert(sym, ItemSig::Trait(sig));
                self.pop_params();
            }
            ItemKind::Union(u) => {
                self.push_declared_params(sym);
                let variants = u
                    .variants
                    .iter()
                    .map(|v| VariantSig {
                        name: v.name.name.clone(),
                        fields: v.fields.iter().map(|f| self.field_sig(f)).collect(),
                        span: v.name.span,
                        carries: if v.has_payload { None } else { self.carried_struct(v) },
                    })
                    .collect();
                let methods = self.collect_struct(sym, &u.methods);
                self.reserved_union_method(&methods);
                self.table.sigs.insert(sym, ItemSig::Union { variants, methods });
                self.pop_params();
            }
            ItemKind::Enum(e) => {
                let backing = match &e.backing {
                    Some(t) => match self.lower_type(t) {
                        Ty::Int(i) => i,
                        other => {
                            if !other.is_error() {
                                let shown = self.name_of(&other);
                                self.error_help(
                                    codes::TYPE_MISMATCH,
                                    t.span,
                                    format!("`{shown}` is not a valid enum backing type"),
                                    "must be an integer type",
                                    "enums are named integers (spec §7.4)",
                                );
                            }
                            IntTy::I32
                        }
                    },
                    None => IntTy::I32,
                };
                let variants = e
                    .variants
                    .iter()
                    .map(|v| VariantSig {
                        name: v.name.name.clone(),
                        fields: Vec::new(),
                        span: v.name.span,
                        carries: None,
                    })
                    .collect();
                let values = e
                    .variants
                    .iter()
                    .map(|v| v.value.as_ref().and_then(const_int_literal))
                    .collect();
                self.table.sigs.insert(sym, ItemSig::Enum { backing, variants, values });
            }
            ItemKind::Fn(f) => {
                let sig = self.fn_sig(f, item.vis, Some(sym));
                self.check_test_attribute(item, f, &sig, sym);
                self.table.sigs.insert(sym, ItemSig::Fn(sig));
            }
            ItemKind::Const(c) => {
                let ty = match &c.ty {
                    Some(t) => self.lower_type(t),
                    // Without an annotation the value decides, by what its literals and
                    // the constants it names make it — defaulted as a literal is (§2.5).
                    None => {
                        let (m, f) = (self.module, self.file);
                        self.const_value_type(&c.value, m, f, 0).defaulted()
                    }
                };
                self.table.sigs.insert(sym, ItemSig::Const(ty));
            }
            ItemKind::Static(s) => {
                let ty = self.lower_type(&s.ty);
                self.table.sigs.insert(sym, ItemSig::Static(ty));
            }
            ItemKind::Impl(_)
            | ItemKind::Macro(_)
            | ItemKind::MacroInvocation(_)
            | ItemKind::Error => {}
        }
    }

    /// `#[test] fn it_adds() { … }` and `#[bench] fn adding() { … }` (spec §19 M14, M15).
    ///
    /// Both are functions a runner calls with nothing and reads nothing back from: a test
    /// either returns or panics, and a benchmark is timed. Taking an argument would leave
    /// the runner deciding what to pass, returning a value would leave it deciding what
    /// the value meant, and a generic one has no single instance to call — so all three
    /// are refused here, where the declaration is, rather than at some later point.
    fn check_test_attribute(&mut self, item: &Item, f: &FnDecl, sig: &FnSig, sym: SymbolId) {
        let which =
            ["test", "bench"].into_iter().find(|w| item.attrs.iter().any(|a| a.name.name == *w));
        let Some(which) = which else { return };
        if item.attrs.iter().any(|a| a.name.name == "test")
            && item.attrs.iter().any(|a| a.name.name == "bench")
        {
            let name = f.name.name.clone();
            self.error_help(
                codes::BAD_TEST,
                f.name.span,
                format!("`{name}` is both a test and a benchmark"),
                "one or the other",
                "a test is run once and asked whether it passed; a benchmark is run many \
                 times and asked how long it took (spec §19 M14, M15)",
            );
            return;
        }

        // Fallibility is checked before the return type, so `!-> i32` is reported as the
        // fallible signature it is rather than as a non-void return.
        let bad = if !sig.params.is_empty() {
            Some((format!("a {which} takes no arguments"), "the runner has nothing to pass"))
        } else if sig.fallible {
            Some((
                format!("a {which} is not fallible"),
                "report the failure with `panic`, which is what a failing one does",
            ))
        } else if !matches!(sig.ret, Ty::Void) {
            Some((
                format!("a {which} returns nothing"),
                "the runner has nothing to do with a value",
            ))
        } else if !sig.generics.is_empty() {
            Some((format!("a {which} has no type parameters"), "there is no instance to run"))
        } else if f.body.is_none() {
            Some((format!("a {which} needs a body"), "there is nothing to run"))
        } else {
            None
        };

        if let Some((message, label)) = bad {
            let name = f.name.name.clone();
            let how = if which == "test" {
                "a test is `#[test] fn name() { … }`; it passes by returning and fails by \
                 panicking (spec §19 M14)"
            } else {
                "a benchmark is `#[bench] fn name() { … }`; the runner times its body over \
                 many iterations (spec §19 M15)"
            };
            self.error_help(
                codes::BAD_TEST,
                f.name.span,
                format!("`{name}` cannot be a {which}: {message}"),
                label,
                how,
            );
            return;
        }
        if which == "test" {
            self.table.tests.push(sym);
        } else {
            self.table.benches.push(sym);
        }
    }

    /// `kind` is every union's own: `s.kind()` names the active variant (spec §7.5).
    fn reserved_union_method(&mut self, methods: &StructSig) {
        // Nothing inherits from a union, so there is nothing to dispatch to (spec §7.5).
        for m in methods.methods.values().filter(|m| m.dispatch != Dispatch::Static) {
            self.error_help(
                codes::RESERVED_UNION_METHOD,
                m.span,
                "a union's method cannot be `virtual` or `override`",
                "nothing inherits from a union",
                "remove `virtual` / `override` (spec §7.3, §7.5)",
            );
        }
        if let Some(m) = methods.methods.get("kind") {
            self.error_help(
                codes::RESERVED_UNION_METHOD,
                m.span,
                "a union cannot declare a method named `kind`",
                "every union already has `kind()`",
                "`s.kind()` names the active variant (spec §7.5); choose another name",
            );
        }
    }

    fn collect_struct(&mut self, sym: SymbolId, members: &[Member]) -> StructSig {
        let mut sig = StructSig {
            // Resolved in pass 1, because a method may return one and be written above it.
            assoc_types: self.table.assoc_decls.get(&sym).cloned().unwrap_or_default(),
            ..StructSig::default()
        };
        for m in members {
            match &m.kind {
                // Removed by the driver before this phase (spec §12.10).
                MemberKind::CfgIf(_) => {}
                MemberKind::Const(c) => {
                    let ty = match &c.ty {
                        Some(t) => self.lower_type(t),
                        None => {
                            self.error_help(
                                codes::NO_CONTEXT_FOR_LITERAL,
                                c.name.span,
                                format!("`{}` needs its type written", c.name.name),
                                "no type",
                                "a constant declared in a type writes its type: \
                                 `const Global: ^mut Allocator = …` (spec §7.3)",
                            );
                            Ty::Error
                        }
                    };
                    sig.consts.push(AssocConst {
                        name: c.name.name.clone(),
                        ty,
                        vis: m.vis,
                        span: c.name.span,
                    });
                }
                MemberKind::Field(f) => {
                    let mut field = self.field_sig(f);
                    // Visibility lives on the member, not the field node.
                    field.vis = m.vis;
                    // A reference cannot be stored in a field (spec §5.2, §18.3). This one
                    // rule is what lets Dazzle skip lifetime analysis entirely.
                    if let Ty::Ref { .. } = field.ty {
                        let shown = self.name_of(&field.ty);
                        self.error_help(
                            codes::REF_IN_FIELD,
                            f.ty.span,
                            format!("a struct field cannot hold `{shown}`"),
                            "references live only on the stack",
                            "use `^T` for a stored pointer — a reference in a field could \
                             outlive what it points at, and Dazzle has no lifetimes to \
                             catch that (spec §5.2)",
                        );
                    }
                    if sig.fields.iter().any(|x| x.name == field.name) {
                        let name = field.name.clone();
                        self.error(
                            codes::DUPLICATE_FIELD,
                            f.name.span,
                            format!("field `{name}` is declared twice"),
                            "duplicate field",
                        );
                    }
                    sig.fields.push(field);
                }
                MemberKind::Fn(f) => {
                    let s = self.fn_sig(f, m.vis, None);
                    // One `virtual` anywhere in the chain makes the whole type
                    // polymorphic, which decides its destructor and whether it can be
                    // memcpy'd (spec §7.3, 18.32). `flatten_bases` carries it downward.
                    if f.dispatch != Dispatch::Static {
                        sig.polymorphic = true;
                    }
                    let name = f.name.name.clone();
                    let taken = sig.overloads.get(&name).map_or(0, Vec::len);
                    let key = if taken == 0 { name.clone() } else { format!("{name}#{taken}") };
                    sig.overloads.entry(name).or_default().push(key.clone());
                    sig.method_keys.push(key.clone());
                    sig.methods.insert(key, s);
                }
                MemberKind::Ctor(f) => {
                    let s = self.fn_sig(f, m.vis, None);
                    sig.ctors.push(s);
                }
                // Already collected, in pass 1.
                MemberKind::TypeAlias { .. } | MemberKind::Error => {}
            }
        }
        sig
    }

    fn duplicate_special(&mut self, span: Span, what: &str) {
        self.error_help(
            codes::DUPLICATE_SPECIAL_MEMBER,
            span,
            format!("`{what}` is declared twice"),
            format!("a struct may declare at most one `{what}`"),
            "the three special members are a constructor, `copy` and `drop`, and there is \
             one of each (spec §6.1)",
        );
    }

    fn field_sig(&mut self, f: &Field) -> FieldSig {
        let mut ty = self.lower_type(&f.ty);
        if f.is_own {
            // `own ^T` is written as a pointer; the parser records `own` separately when
            // it appears as a field modifier.
            if let Ty::Ptr { mutable, inner } = ty {
                ty = Ty::OwnPtr { mutable, inner };
            }
        }
        FieldSig {
            name: f.name.name.clone(),
            ty,
            vis: Vis::Private,
            has_default: f.default.is_some(),
            span: f.name.span,
            // Dazzle has no bit-fields of its own; only an imported C struct brings them.
            bitfield: false,
            declared_in: None,
        }
    }

    /// A function's own parameters are in scope for its signature and its body; any
    /// enclosing type's are already on the stack (spec §8.1).
    ///
    /// `sym` is `Some` for a free function, whose parameters pass 0 already allocated;
    /// `None` for a method, which allocates its own here.
    fn fn_sig(&mut self, f: &FnDecl, vis: Vis, sym: Option<SymbolId>) -> FnSig {
        let generics = match sym {
            Some(sym) => self.table.type_params.get(&sym).cloned().unwrap_or_default(),
            None => {
                // A method's parameters have no symbol of their own for pass 1 to fill in,
                // so their bounds are resolved here — every trait exists by the time member
                // signatures are read. Without this `fn twice<T: Copy>` lost its bound.
                let mut defs = self.declare_params(&f.generics);
                self.push_params(&defs);
                for (def, p) in defs.iter_mut().zip(&f.generics.params) {
                    let self_ty = Ty::Param(def.param.clone());
                    def.bounds =
                        p.bounds.iter().filter_map(|b| self.trait_bound(b, &self_ty)).collect();
                    def.default = p.default.as_ref().map(|t| self.lower_type(t));
                    self.table.param_bounds.insert(def.param.id, def.bounds.clone());
                }
                self.pop_params();
                defs
            }
        };
        self.push_params(&generics);
        let sig = self.fn_sig_inner(f, vis, generics);
        self.pop_params();
        sig
    }

    fn fn_sig_inner(&mut self, f: &FnDecl, vis: Vis, generics: Vec<ParamDef>) -> FnSig {
        let (params, variadic) = self.param_sigs(&f.params);
        let ret = f.ret.as_ref().map(|r| self.lower_type(r)).unwrap_or(Ty::Void);
        FnSig {
            // Only an imported function needs a cast at the boundary (spec §15.2).
            ret_cast: false,
            dispatch: f.dispatch,
            params,
            ret,
            fallible: f.fallible,
            takes_self: f.self_param.map(|s| s.is_mut),
            // The receiver is the only place a method says it mutates (spec §7.3, §18.34).
            is_mut: f.self_param.is_some_and(|s| s.is_mut),
            is_unsafe: f.is_unsafe,
            vis,
            span: f.name.span,
            generics,
            has_body: f.body.is_some(),
            variadic,
        }
    }

    /// A declaration's parameters as a signature sees them, and which one is variadic.
    /// `xs: ..T` becomes a `[]T` — read-only, a view of the caller's arguments — and has no
    /// default; there is at most one, and the parameters after it are passed by name
    /// (spec §7.2). A misplaced one is reported and then treated as the slice it would be,
    /// so the body still checks.
    pub(crate) fn param_sigs(
        &mut self,
        params: &[dz_ast::Param],
    ) -> (Vec<ParamSig>, Option<usize>) {
        let mut variadic = None;
        let mut out = Vec::with_capacity(params.len());
        for (i, p) in params.iter().enumerate() {
            let mut ty = self.lower_type(&p.ty);
            let mut is_variadic = false;
            if p.variadic {
                ty = Ty::Slice { mutable: false, elem: Rc::new(ty) };
                if variadic.is_some() {
                    self.error_help(
                        codes::BAD_VARIADIC,
                        p.span,
                        format!("`{}` is a second variadic parameter", p.name.name),
                        "one variadic parameter already takes the rest",
                        "a function has at most one: the arguments after the fixed ones are its \
                         (spec §7.2)",
                    );
                } else if let Some(d) = &p.default {
                    self.error_help(
                        codes::BAD_VARIADIC,
                        d.span,
                        format!("`{}` cannot have a default", p.name.name),
                        "a default on a variadic parameter",
                        "leaving its arguments out already gives it none (spec §7.2)",
                    );
                } else {
                    variadic = Some(i);
                    is_variadic = true;
                }
            }
            out.push(ParamSig {
                name: p.name.name.clone(),
                ty,
                has_default: p.default.is_some() || is_variadic,
                caller_default: caller_default(p.default.as_ref()),
                c_cast: None,
            });
        }
        (out, variadic)
    }

    pub(crate) fn symbol_of(&self, item: &Item) -> Option<SymbolId> {
        let name = match &item.kind {
            // Removed by the driver before this phase (spec §12.10).
            ItemKind::CfgIf(_) => return None,
            ItemKind::Fn(f) => &f.name.name,
            ItemKind::Struct(s) => &s.name.name,
            ItemKind::Union(u) => &u.name.name,
            ItemKind::Enum(e) => &e.name.name,
            ItemKind::Trait(t) => &t.name.name,
            ItemKind::Const(c) => &c.name.name,
            ItemKind::Static(s) => &s.name.name,
            ItemKind::Macro(m) => &m.name.name,
            ItemKind::Impl(_) | ItemKind::MacroInvocation(_) | ItemKind::Error => return None,
        };
        self.res.lookup(self.module, name)
    }
}

/// Field visibility comes from the member, not the field node, so it is patched in after
/// signature collection walks the members.
pub(crate) fn builtin_type(name: &str) -> Option<Ty> {
    if let Some(i) = IntTy::from_name(name) {
        return Some(Ty::Int(i));
    }
    Some(match name {
        "f32" => Ty::Float(FloatTy::F32),
        "f64" => Ty::Float(FloatTy::F64),
        "bool" => Ty::Bool,
        "char" => Ty::Char,
        "str" => Ty::Str,
        "cstr" => Ty::CStr,
        "void" => Ty::Void,
        "Self" => Ty::SelfTy,
        // `Error` and `SourceLocation` are provided by the language, not the stdlib
        // (spec §9.2, §12.7), so they are spellable without an import.
        other => Ty::Builtin(ty::BuiltinTy::from_name(other)?),
    })
}

/// An integer written as a literal, with an optional leading `-` (spec §7.4).
///
/// A signed enum needs `Less = -1`, and unary minus is an *operator* rather than part of
/// the literal (§2.5), so a bare `ExprKind::Int` match would never see it.
pub(crate) fn const_int_literal(e: &Expr) -> Option<i128> {
    match &e.kind {
        ExprKind::Int(text) => parse_int_literal(text),
        ExprKind::Unary { op: UnaryOp::Neg, operand } => const_int_literal(operand).map(|n| -n),
        _ => None,
    }
}

/// Parse an integer literal's numeric value, ignoring separators and any type suffix.
/// The type a number literal's suffix fixes — `5u8` is a `u8`, `1.5f` and `2f32` are `f32`
/// — or `None` for a literal without one, which stays untyped until its context types it
/// (spec §2.5). The lexer accepted only these suffixes, and only integer ones after a radix
/// prefix, so a hex digit `f` is never mistaken for one.
pub(crate) fn literal_suffix_type(text: &str) -> Option<Ty> {
    let prefixed = text.len() > 1
        && text.starts_with('0')
        && matches!(text.as_bytes()[1], b'x' | b'X' | b'o' | b'O' | b'b' | b'B');
    const INTS: &[(&str, IntTy)] = &[
        ("usize", IntTy::Usize),
        ("isize", IntTy::Isize),
        ("i16", IntTy::I16),
        ("i32", IntTy::I32),
        ("i64", IntTy::I64),
        ("u16", IntTy::U16),
        ("u32", IntTy::U32),
        ("u64", IntTy::U64),
        ("i8", IntTy::I8),
        ("u8", IntTy::U8),
    ];
    if let Some((_, i)) = INTS.iter().find(|(s, _)| text.ends_with(s)) {
        return Some(Ty::Int(*i));
    }
    if prefixed {
        return None;
    }
    if text.ends_with("f64") {
        Some(Ty::Float(FloatTy::F64))
    } else if text.ends_with("f32") || text.ends_with('f') {
        Some(Ty::Float(FloatTy::F32))
    } else {
        None
    }
}

pub(crate) fn parse_int_literal(text: &str) -> Option<i128> {
    let cleaned: String = text.chars().filter(|c| *c != '_').collect();
    let (digits, radix) =
        if let Some(rest) = cleaned.strip_prefix("0x").or(cleaned.strip_prefix("0X")) {
            (rest, 16)
        } else if let Some(rest) = cleaned.strip_prefix("0o").or(cleaned.strip_prefix("0O")) {
            (rest, 8)
        } else if let Some(rest) = cleaned.strip_prefix("0b").or(cleaned.strip_prefix("0B")) {
            (rest, 2)
        } else {
            (cleaned.as_str(), 10)
        };
    // Strip a trailing type suffix such as `u32`.
    let end = digits
        .char_indices()
        .find(|(_, c)| !c.is_digit(radix) && *c != '_')
        .map(|(i, _)| i)
        .unwrap_or(digits.len());
    i128::from_str_radix(&digits[..end], radix).ok()
}

/// Decode a string or character literal into the bytes it denotes: strip the quotes and
/// apply the escapes of spec §2.4.
///
/// The AST keeps literals as the exact source text, because the formatter has to reproduce
/// what was written. The HIR keeps the value, because everything downstream wants the
/// bytes. The lexer has already rejected malformed escapes, so anything unrecognised here
/// is passed through rather than reported a second time.
pub(crate) fn decode_string_literal(text: &str) -> String {
    // A string's quotes or a character's — not both, or `"'a'"` lost its own.
    let body = text
        .strip_prefix('"')
        .and_then(|t| t.strip_suffix('"'))
        .or_else(|| text.strip_prefix('\'').and_then(|t| t.strip_suffix('\'')))
        .unwrap_or(text);
    decode_escapes(body)
}

/// The escape sequences of a literal's text, between its delimiters, decoded.
pub(crate) fn decode_escapes(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    let mut chars = body.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('0') => out.push('\0'),
            Some('\\') => out.push('\\'),
            Some('"') => out.push('"'),
            Some('\'') => out.push('\''),
            Some('x') => {
                let hex: String = chars.by_ref().take(2).collect();
                match u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                    Some(c) => out.push(c),
                    None => out.push('\u{FFFD}'),
                }
            }
            Some('u') => {
                let mut hex = String::new();
                for h in chars.by_ref() {
                    match h {
                        '{' => {}
                        '}' => break,
                        _ => hex.push(h),
                    }
                }
                match u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                    Some(c) => out.push(c),
                    None => out.push('\u{FFFD}'),
                }
            }
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

/// Strip the delimiters of a raw string literal, which has no escapes (spec §2.4).
pub(crate) fn decode_raw_string_literal(text: &str) -> String {
    text.strip_prefix('"').and_then(|t| t.strip_suffix('"')).unwrap_or(text).to_owned()
}

/// Parse a float literal's value, ignoring separators and any type suffix.
pub(crate) fn parse_float_literal(text: &str) -> f64 {
    let cleaned: String = text.chars().filter(|c| *c != '_').collect();
    let end = cleaned
        .char_indices()
        .find(|(_, c)| !c.is_ascii_digit() && !matches!(c, '.' | 'e' | 'E' | '+' | '-'))
        .map(|(i, _)| i)
        .unwrap_or(cleaned.len());
    cleaned[..end].parse().unwrap_or(0.0)
}
