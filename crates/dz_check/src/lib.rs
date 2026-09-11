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

use std::collections::HashMap;
use std::rc::Rc;

use dz_ast::*;
use dz_diag::{codes, Diagnostic, Diagnostics};
use dz_hir::Hir;
use dz_project::{ModuleId, Workspace};
use dz_resolve::{ParsedFile, Resolution, SymbolId, SymbolKind};
use dz_span::{FileId, Span};

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
        params
            .iter()
            .map(|p| p.param.id)
            .zip(self.args.iter().cloned())
            .collect()
    }
}

#[derive(Clone, Debug)]
pub struct FieldSig {
    pub name: String,
    pub ty: Ty,
    pub vis: Vis,
    pub has_default: bool,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct ParamSig {
    pub name: String,
    pub ty: Ty,
    pub has_default: bool,
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
}

impl FnSig {
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
    pub fields: Vec<FieldSig>,
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
}

#[derive(Clone, Debug)]
pub struct VariantSig {
    pub name: String,
    pub fields: Vec<FieldSig>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum ItemSig {
    Struct(StructSig),
    Union {
        variants: Vec<VariantSig>,
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
}

/// Everything the body checker needs to know about declarations.
pub struct TypeTable {
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
    /// Every `impl Trait for Type`, in declaration order (spec §7.6).
    impls: Vec<ImplSig>,
    /// The `impl Drop` / `impl Copy` of each struct that has one (spec §6.1). They are
    /// implementations like any other, but they become the struct's C++ destructor and
    /// copy constructor, so each struct has at most one of each and they are found by struct.
    drop_impls: HashMap<SymbolId, usize>,
    copy_impls: HashMap<SymbolId, usize>,
    /// The prelude's traits by name, for the ones the primitives implement (spec §11.2).
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
        let Some((idx, imap)) = self.impl_of(r, self_ty) else { return map };
        let imp = &self.impls[idx];
        for (name, param) in self.assoc_params(r.trait_sym) {
            if let Some(chosen) = imp.assoc_types.iter().find(|a| &a.name == name) {
                if let Some(ty) = &chosen.ty {
                    map.insert(param.id, ty::substitute_with(ty, &imap, Some(self_ty)));
                }
            }
        }
        map
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
                    ty: ty::substitute_with(&p.ty, &map, Some(self_ty)),
                    has_default: p.has_default,
                })
                .collect(),
            ret: ty::substitute_with(&sig.ret, &map, Some(self_ty)),
            ..sig.clone()
        })
    }

    pub fn struct_sig(&self, id: SymbolId) -> Option<&StructSig> {
        match self.sigs.get(&id)? {
            ItemSig::Struct(s) | ItemSig::Trait(s) => Some(s),
            _ => None,
        }
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
                Some(ItemSig::Union { variants }) => variants.iter().any(|v| {
                    self.field_types(*id, args, &v.fields).iter().any(|t| self.needs_drop(t))
                }),
                _ => false,
            },
            // Inside a generic body `T` may be anything, so it is assumed to need dropping.
            // That is the conservative answer, and the one that keeps flow analysis honest
            // about a body checked once for every `T` it will ever be given (spec §8.2).
            Ty::Param(_) => true,
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

    /// Is `p` bounded by `Copy`, directly or through a supertrait (spec §8.2)?
    pub fn param_is_copy(&self, p: &ty::Param) -> bool {
        let Some(copy) = self.copy_trait else { return false };
        self.trait_closure(self.bounds_of(p), &Ty::Param(p.clone()))
            .iter()
            .any(|r| r.trait_sym == copy)
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
                        if s.copy.is_some() {
                            // An `impl Copy` makes the type copyable wherever it applies:
                            // `impl<T: Copy> Copy for Buffer<T>` covers `Buffer<f32>` and
                            // not `Buffer<File>`, which is move-only (spec §6.1, §8.2).
                            match self.copy_impls.get(id) {
                                Some(&i) => self.impl_applies(&self.impls[i], ty).is_some(),
                                None => true,
                            }
                        } else if s.drop.is_some() {
                            false
                        } else {
                            // "no `own` field, and every field is copyable" (spec §6.3),
                            // read through the instantiation so `Pair<Owner>` is move-only.
                            self.field_types(*id, args, &s.fields).iter().all(|t| {
                                !matches!(t, Ty::OwnPtr(_)) && self.copyable_guarded(t, stack)
                            })
                        }
                    }
                    Some(ItemSig::Union { variants }) => variants.iter().all(|v| {
                        self.field_types(*id, args, &v.fields)
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
            _ => true,
        }
    }

    /// A declaration's field types as seen from one instantiation of it (spec §8.4).
    pub fn field_types(&self, sym: SymbolId, args: &[Ty], fields: &[FieldSig]) -> Vec<Ty> {
        let map: HashMap<u32, Ty> = self
            .type_params(sym)
            .iter()
            .map(|p| p.param.id)
            .zip(args.iter().cloned())
            .collect();
        fields.iter().map(|f| ty::substitute(&f.ty, &map)).collect()
    }

    /// Spec §5.5's `is_trivially_copyable`: no `drop`, no `copy`, and every field trivially
    /// copyable — a type `memcpy` may duplicate. Stricter than [`TypeTable::copyable`],
    /// which a user-written `copy` satisfies.
    pub fn trivially_copyable(&self, ty: &Ty) -> bool {
        match ty {
            Ty::Named { sym, args } => match self.get(*sym) {
                Some(ItemSig::Struct(s)) => {
                    s.drop.is_none()
                        && s.copy.is_none()
                        && self.field_types(*sym, args, &s.fields).iter().all(|t| {
                            !matches!(t, Ty::OwnPtr(_)) && self.trivially_copyable(t)
                        })
                }
                Some(ItemSig::Union { variants }) => variants.iter().all(|v| {
                    self.field_types(*sym, args, &v.fields)
                        .iter()
                        .all(|t| self.trivially_copyable(t))
                }),
                _ => true,
            },
            Ty::Array { elem, .. } => self.trivially_copyable(elem),
            Ty::Tuple(parts) => parts.iter().all(|t| self.trivially_copyable(t)),
            // Unknown until monomorphization, where the query is answered for real.
            Ty::Param(_) => false,
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
            Ty::Named { sym: id, .. } => match self.get(*id) {
                Some(ItemSig::Struct(s)) if s.copy.is_some() => {
                    "its `impl Copy` does not apply to these type arguments (spec §8.2)"
                        .to_owned()
                }
                Some(ItemSig::Struct(s)) if s.drop.is_some() => {
                    "it declares a `drop`, so copying it would release the same resource \
                     twice"
                        .to_owned()
                }
                Some(ItemSig::Struct(s)) => {
                    match s.fields.iter().find(|f| matches!(f.ty, Ty::OwnPtr(_))) {
                        Some(f) => format!(
                            "its field `{}` is `own`, so the struct is responsible for                              what it points at",
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
pub fn check(
    ws: &Workspace,
    res: &Resolution,
    files: &HashMap<ModuleId, Vec<ParsedFile>>,
) -> CheckResult {
    let mut cx = Ctx {
        ws,
        res,
        diags: Diagnostics::new(),
        table: TypeTable {
            sigs: HashMap::new(),
            type_params: HashMap::new(),
            param_bounds: HashMap::new(),
            assoc_params: HashMap::new(),
            assoc_decls: HashMap::new(),
            declared_traits: HashMap::new(),
            copy_trait: None,
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
    for (i, s) in res.symbols.iter().enumerate() {
        if s.module == res.prelude && s.kind == SymbolKind::Trait {
            cx.table.prelude_names.insert(SymbolId(i as u32), s.name.clone());
        }
    }

    // Four passes, in strict dependency order. Each exists because the one before it
    // supplies something the next cannot do without, whatever order declarations appear in
    // — which is the point, since order never matters inside a module (spec §3.2).
    //
    //   0. Generic parameters and associated types: what names *are* types.
    //   1. Bounds and trait lists: `Mul<f32>` needs `Mul`'s parameter list to check its
    //      argument count, and the trait may be declared after the struct using it.
    //   2. Signatures: a field of type `Buffer<f32>` needs both of the above.
    //   3. Bodies.
    fn run(
        ws: &Workspace,
        files: &HashMap<ModuleId, Vec<ParsedFile>>,
        cx: &mut Ctx,
        f: impl Fn(&mut Ctx, &Item),
    ) {
        for module in ws.modules() {
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
    }
    run(ws, files, &mut cx, |cx, it| cx.collect_type_params(it));
    run(ws, files, &mut cx, |cx, it| cx.collect_trait_refs(it));
    run(ws, files, &mut cx, |cx, it| cx.collect_item(it));
    // `impl Drop` and `impl Copy` fill in their struct's special members, which every
    // later question about ownership reads — so after every signature exists, and before
    // any body is checked (spec §6.1).
    cx.attach_special_impls();
    run(ws, files, &mut cx, check::item);

    cx.diags.sort_by_position();
    CheckResult {
        table: cx.table,
        hir: cx.hir,
        diagnostics: cx.diags,
    }
}

pub(crate) struct Ctx<'a> {
    pub(crate) ws: &'a Workspace,
    pub(crate) res: &'a Resolution,
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
        self.diags
            .push(Diagnostic::error(code, msg.into()).at(self.file, span, label.into()));
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
    let name_of = |t: &Ty| type_name(res, t);
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
        Ty::Ptr(i) => format!("^{}", name_of(i)),
        Ty::OwnPtr(i) => format!("own ^{}", name_of(i)),
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
        Ty::Tuple(elems) => format!(
            "({})",
            elems.iter().map(&name_of).collect::<Vec<_>>().join(", ")
        ),
        Ty::Fn { params, ret } => format!(
            "fn({}) -> {}",
            params.iter().map(&name_of).collect::<Vec<_>>().join(", "),
            name_of(ret)
        ),
        Ty::Builtin(b) => b.name().to_owned(),
        Ty::Named { sym: id, args } => {
            let name = res.symbol(*id).name.clone();
            if args.is_empty() {
                name
            } else {
                // As written in source: `Buffer<f32>`, never a mangled instantiation name.
                // Diagnostics quote user syntax (principle 4, §1).
                let args = args.iter().map(&name_of).collect::<Vec<_>>().join(", ");
                format!("{name}<{args}>")
            }
        }
        Ty::Variant(id, i) => {
            let owner = res.symbol(*id);
            let variant = owner.members.iter().filter(|m| m.sort == dz_resolve::MemberSort::Variant).nth(*i);
            match variant {
                Some(v) => format!("{}::{}", owner.name, v.name),
                None => owner.name.clone(),
            }
        }
        Ty::SelfTy => "Self".to_owned(),
        Ty::Param(p) => p.name.to_string(),
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
        let sym = match path.segments.len() {
            1 => self.res.lookup_in_file(self.module, self.file, &last.name.name),
            _ => {
                let first = &path.segments[0].name.name;
                let target = self.res.module_alias(self.file, first)?;
                self.res.lookup(target, &last.name.name)
            }
        }?;
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
        let first = &p.segments.first()?.name.name;
        let sym = match p.segments.len() {
            1 => self.res.lookup_in_file(module, file, first),
            2 => {
                let target = self.res.module_alias(file, first)?;
                self.res.lookup(target, &p.segments[1].name.name)
            }
            _ => None,
        }?;
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
            ExprKind::Int(_) => Ty::IntLit,
            ExprKind::Float(_) => Ty::FloatLit,
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
        ty::substitute(t, &map)
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

    fn prelude_decl(&self, name: &str, kind: SymbolKind) -> Option<SymbolId> {
        let sym = self.res.lookup(self.res.prelude, name)?;
        (self.res.symbol(sym).kind == kind).then_some(sym)
    }

    /// What `Self` means inside `sym`'s body: the declaration applied to its own
    /// parameters, so `Self` inside `Buffer<T>` is `Buffer<T>` and not a bare `Buffer`.
    pub(crate) fn self_type_of(&self, sym: SymbolId) -> Ty {
        let args = self
            .table
            .type_params(sym)
            .iter()
            .map(|p| Ty::Param(p.param.clone()))
            .collect();
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
                format!("`{name}` is declared with {wanted} type {}",
                    plural(wanted, "parameter")),
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
        let scope = defs
            .iter()
            .map(|d| (d.name().to_owned(), Ty::Param(d.param.clone())))
            .collect();
        self.param_scopes.push(scope);
    }

    pub(crate) fn pop_params(&mut self) {
        self.param_scopes.pop();
    }

    pub(crate) fn param_in_scope(&self, name: &str) -> Option<Ty> {
        self.param_scopes.iter().rev().find_map(|s| s.get(name).cloned())
    }

    // --- resolving syntax types into `Ty` ---------------------------------

    pub(crate) fn lower_type(&mut self, t: &Type) -> Ty {
        match &t.kind {
            TypeKind::Path(p) => self.lower_path_type(p, t.span),
            TypeKind::Ptr(i) => Ty::Ptr(Rc::new(self.lower_type(i))),
            TypeKind::OwnPtr(i) => Ty::OwnPtr(Rc::new(self.lower_type(i))),
            TypeKind::Ref { mutable, inner } => Ty::Ref {
                mutable: *mutable,
                inner: Rc::new(self.lower_type(inner)),
            },
            TypeKind::Optional(i) => Ty::Optional(Rc::new(self.lower_type(i))),
            TypeKind::Array { size, elem } => {
                let elem = Rc::new(self.lower_type(elem));
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
            TypeKind::Slice { mutable, elem } => Ty::Slice {
                mutable: *mutable,
                elem: Rc::new(self.lower_type(elem)),
            },
            TypeKind::Tuple(elems) => Ty::Tuple(elems.iter().map(|e| self.lower_type(e)).collect()),
            TypeKind::Fn { params, ret } => Ty::Fn {
                params: params.iter().map(|p| self.lower_type(p)).collect(),
                ret: Rc::new(ret.as_ref().map(|r| self.lower_type(r)).unwrap_or(Ty::Void)),
            },
            TypeKind::SelfType => Ty::SelfTy,
            TypeKind::Error => Ty::Error,
        }
    }

    fn lower_path_type(&mut self, p: &Path, span: Span) -> Ty {
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

        // `module::Type`
        if let Some(target) = self.res.module_alias(self.file, name) {
            if let Some(second) = p.segments.get(1) {
                if let Some(sym) = self.res.lookup(target, &second.name.name) {
                    return self.named_or_error(sym, second, span);
                }
            }
        }
        Ty::Error
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
                    format!(
                        "`{name}` is {} and cannot be used as a type",
                        other.describe_a()
                    ),
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
            let bounds = bounds
                .iter()
                .filter_map(|b| self.trait_bound(b, &Ty::SelfTy))
                .collect();
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
            def.bounds =
                p.bounds.iter().filter_map(|b| self.trait_bound(b, &self_ty)).collect();
            def.default = p.default.as_ref().map(|t| self.lower_type(t));
            self.table.param_bounds.insert(def.param.id, def.bounds.clone());
        }
        self.table.type_params.insert(sym, defs);
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
                    })
                    .collect();
                self.table.sigs.insert(sym, ItemSig::Union { variants });
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
                    })
                    .collect();
                let values = e
                    .variants
                    .iter()
                    .map(|v| v.value.as_ref().and_then(const_int_literal))
                    .collect();
                self.table
                    .sigs
                    .insert(sym, ItemSig::Enum { backing, variants, values });
            }
            ItemKind::Fn(f) => {
                let sig = self.fn_sig(f, item.vis, Some(sym));
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
            ItemKind::Impl(_)
            | ItemKind::Macro(_)
            | ItemKind::MacroInvocation { .. }
            | ItemKind::Error => {}
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
            if let Ty::Ptr(inner) = ty {
                ty = Ty::OwnPtr(inner);
            }
        }
        FieldSig {
            name: f.name.name.clone(),
            ty,
            vis: Vis::Private,
            has_default: f.default.is_some(),
            span: f.name.span,
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
            None => self.declare_params(&f.generics),
        };
        self.push_params(&generics);
        let sig = self.fn_sig_inner(f, vis, generics);
        self.pop_params();
        sig
    }

    fn fn_sig_inner(&mut self, f: &FnDecl, vis: Vis, generics: Vec<ParamDef>) -> FnSig {
        let params = f
            .params
            .iter()
            .map(|p| ParamSig {
                name: p.name.name.clone(),
                ty: self.lower_type(&p.ty),
                has_default: p.default.is_some(),
            })
            .collect();
        let ret = f
            .ret
            .as_ref()
            .map(|r| self.lower_type(r))
            .unwrap_or(Ty::Void);
        FnSig {
            params,
            ret,
            fallible: f.fallible,
            takes_self: f.self_param.map(|s| s.is_mut),
            is_mut: f.is_mut,
            is_unsafe: f.is_unsafe,
            vis,
            span: f.name.span,
            generics,
            has_body: f.body.is_some(),
        }
    }

    pub(crate) fn symbol_of(&self, item: &Item) -> Option<SymbolId> {
        let name = match &item.kind {
            ItemKind::Fn(f) => &f.name.name,
            ItemKind::Struct(s) => &s.name.name,
            ItemKind::Union(u) => &u.name.name,
            ItemKind::Enum(e) => &e.name.name,
            ItemKind::Trait(t) => &t.name.name,
            ItemKind::Const(c) => &c.name.name,
            ItemKind::Macro(m) => &m.name.name,
            ItemKind::Impl(_) | ItemKind::MacroInvocation { .. } | ItemKind::Error => return None,
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
        other => match ty::BuiltinTy::from_name(other) {
            Some(b) => Ty::Builtin(b),
            None => return None,
        },
    })
}

/// An integer written as a literal, with an optional leading `-` (spec §7.4).
///
/// A signed enum needs `Less = -1`, and unary minus is an *operator* rather than part of
/// the literal (§2.5), so a bare `ExprKind::Int` match would never see it.
pub(crate) fn const_int_literal(e: &Expr) -> Option<i128> {
    match &e.kind {
        ExprKind::Int(text) => parse_int_literal(text),
        ExprKind::Unary { op: UnaryOp::Neg, operand } => {
            const_int_literal(operand).map(|n| -n)
        }
        _ => None,
    }
}


/// Parse an integer literal's numeric value, ignoring separators and any type suffix.
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
    let body = text
        .strip_prefix('"')
        .and_then(|t| t.strip_suffix('"'))
        .unwrap_or(text);
    let body = body
        .strip_prefix('\'')
        .and_then(|t| t.strip_suffix('\''))
        .unwrap_or(body);
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
    text.strip_prefix('"')
        .and_then(|t| t.strip_suffix('"'))
        .unwrap_or(text)
        .to_owned()
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

