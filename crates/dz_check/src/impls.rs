//! Implementations: `impl Trait for Type` (spec §7.6).
//!
//! An `impl` is the only way a trait is implemented. It may be written in the trait's
//! module or the type's — the orphan rule — which is what lets code that never saw the
//! type add behaviour to it (an editor module implementing `Inspect` for an engine's
//! structs, or for `f32`), while guaranteeing that any code which can see both the trait
//! and the type can see the one implementation that joins them.
//!
//! This file holds the signature passes for implementations and the queries every later
//! phase asks: which implementations apply to a type, whether a type implements a trait,
//! and which implementation a call reaches.

use std::collections::HashMap;

use dz_ast::*;
use dz_diag::codes;
use dz_resolve::SymbolId;

use crate::ty::{self, Ty};
use crate::{Ctx, ImplSig, ItemSig, TraitRef, TypeTable};

// ---------------------------------------------------------------------------
// signature passes
// ---------------------------------------------------------------------------

impl Ctx<'_> {
    /// Pass 1: what the implementation is — its trait, its type, its parameters' bounds,
    /// and its associated-type choices (spec §7.6, §8.5).
    pub(crate) fn impl_header(&mut self, item: &Item, i: &ImplDecl) {
        let key = (self.file, item.span.start);
        let mut defs = self.impl_params.get(&key).cloned().unwrap_or_default();
        self.push_params(&defs);
        let self_ty = self.lower_type(&i.self_ty);
        // A parameter that reaches the type's `view` parameter may be given a view, so the
        // implementation must treat it as one (spec §6.7): `impl<view I: Iterate> Iterate
        // for Numbered<I>`. Otherwise its body could store an `I` anywhere.
        if let Ty::Named { sym, args } = &self_ty {
            let wanted: Vec<bool> =
                self.table.type_params(*sym).iter().map(|p| p.param.view).collect();
            for (arg, view) in args.iter().zip(wanted) {
                let Ty::Param(p) = arg else { continue };
                let Some(def) = defs.iter().find(|d| d.param.id == p.id) else { continue };
                if view && !p.view {
                    let name = self.res.symbol(*sym).name.clone();
                    let pn = def.name().to_owned();
                    self.error_help(
                        codes::REF_IN_FIELD,
                        def.span,
                        format!("`{pn}` reaches `{name}`'s `view` parameter, so it may be a view"),
                        "not declared `view`",
                        format!(
                            "write `impl<view {pn}: …>`, so the implementation treats a value of \
                             `{pn}` as the view it may be (spec §6.7)"
                        ),
                    );
                }
            }
        }
        for (def, p) in defs.iter_mut().zip(&i.generics.params) {
            let on = Ty::Param(def.param.clone());
            def.bounds = p.bounds.iter().filter_map(|b| self.trait_bound(b, &on)).collect();
            self.table.param_bounds.insert(def.param.id, def.bounds.clone());
        }
        let trait_ref = i.trait_path.as_ref().and_then(|p| self.trait_bound(p, &self_ty));
        let mut assoc_types = self.collect_assoc_types(&i.items);
        self.pop_params();
        // `type Output = Self` means the type implemented for.
        let empty = HashMap::new();
        for a in &mut assoc_types {
            a.ty = a.ty.as_ref().map(|t| ty::substitute_with(t, &empty, Some(&self_ty)));
        }
        // An unresolved trait or type has been reported; there is nothing to register.
        let Some(trait_ref) = trait_ref else { return };
        if self_ty.is_error() {
            return;
        }
        let span = i.trait_path.as_ref().map_or(item.span, |p| p.span);
        let imp = ImplSig {
            trait_ref,
            self_ty,
            generics: defs,
            methods: HashMap::new(),
            method_order: Vec::new(),
            assoc_types,
            module: self.module,
            file: self.file,
            span,
            block_span: item.span,
        };
        // An implementation written where it may not be is reported and never registered:
        // one for every type would otherwise make every type satisfy the trait, and every
        // later question about the trait would be answered wrongly.
        if !self.impl_placement_ok(&imp) {
            return;
        }
        self.impl_at.insert(key, self.table.impls.len());
        self.table.impls.push(imp);
    }

    /// Where an implementation may be written (spec §6.1, §7.6). Reports, and returns
    /// `false`, when it may not.
    fn impl_placement_ok(&mut self, imp: &ImplSig) -> bool {
        let trait_sym = imp.trait_ref.trait_sym;
        let trait_name = self.res.symbol(trait_sym).name.clone();
        let me = self.name_of(&imp.self_ty);
        let is_drop = Some(trait_sym) == self.prelude_trait("Drop");
        let special = is_drop || Some(trait_sym) == self.prelude_trait("Copy");
        if let Ty::Param(_) = imp.self_ty {
            self.error_help(
                codes::ORPHAN_IMPL,
                imp.span,
                format!("`{trait_name}` cannot be implemented for every type at once"),
                "this is any type at all",
                format!(
                    "implement `{trait_name}` for each type that needs it, or for a generic \
                     type such as `List<T>` — an implementation for every type would overlap \
                     all the others (spec §7.6)"
                ),
            );
            return false;
        }
        // A method call reaches through a pointer or reference to what it points at (§5.2),
        // so an implementation on the pointer type itself could only be reached through a
        // bound — and the call there is emitted on the pointee too (T-0080).
        if let Ty::Ptr { inner, .. } | Ty::OwnPtr { inner, .. } | Ty::Ref { inner, .. } =
            &imp.self_ty
        {
            let inner = self.name_of(inner);
            self.error_help(
                codes::ORPHAN_IMPL,
                imp.span,
                format!("`{trait_name}` cannot be implemented for a pointer or reference type"),
                format!("`{me}` is a pointer or reference"),
                format!(
                    "a method call on `{me}` reaches through to `{inner}`, so this could never \
                     be called; implement `{trait_name}` for `{inner}`, or wrap the pointer in a \
                     struct of your own and implement it for that (spec §7.6)"
                ),
            );
            return false;
        }
        if special {
            if self.special_owner(imp).is_none() {
                self.error_help(
                    codes::ORPHAN_IMPL,
                    imp.span,
                    format!(
                        "`{trait_name}` can only be implemented for a struct, in its own module"
                    ),
                    format!("not allowed for `{me}` here"),
                    format!(
                        "`{trait_name}` becomes the struct's C++ {}, so it is part of the \
                         struct's definition and covers every instance — `impl<T> {trait_name} \
                         for List<T>`, not one instance (spec §6.1)",
                        if is_drop { "destructor" } else { "copy constructor" }
                    ),
                );
                return false;
            }
            if is_drop && imp.generics.iter().any(|g| !g.bounds.is_empty()) {
                self.error_help(
                    codes::ORPHAN_IMPL,
                    imp.span,
                    "`impl Drop` applies to every instance, so its parameters take no bounds",
                    "bounded here",
                    "a destructor cannot exist for some instances and not others (spec §6.1)",
                );
                return false;
            }
            return true;
        }
        if !self.impl_is_local(imp) {
            let (label, help) = if imp.trait_ref.args.is_empty() {
                (
                    "neither is declared in this module",
                    "otherwise two libraries could both implement it, and a program using \
                     both could not choose between them; to implement someone else's trait \
                     for someone else's type, wrap the type in a struct of your own \
                     (spec §7.6)",
                )
            } else {
                (
                    "neither it, the type, nor a type it is given is declared in this module",
                    "a trait taking type arguments may also be implemented where one of them \
                     is declared, as `impl ImplicitAs<Vec2> for rl::Vector2` is with `Vec2`; \
                     otherwise wrap the type in a struct of your own (spec §7.6)",
                )
            };
            self.error_help(
                codes::ORPHAN_IMPL,
                imp.span,
                format!("`impl {trait_name} for {me}` belongs with `{trait_name}` or with `{me}`"),
                label,
                help,
            );
            return false;
        }
        true
    }

    /// The orphan rule (spec §7.6): an implementation is written with its trait, or where
    /// one of the types it joins is declared — `Self`, then each of the trait's arguments,
    /// in order. So the module that declares `Vec2` may write `impl ImplicitAs<Vec2> for
    /// rl::Vector2`. Two modules could only both write one implementation if each named
    /// the other's type, which would make their dependencies a cycle (§3.2).
    ///
    /// A bare type parameter before the local type ends the search, as in Rust: `impl<T>
    /// Pair<T, Mine> for Theirs` would cover `Pair<Yours, Mine>`, which the module declaring
    /// `Yours` may also write without a cycle.
    fn impl_is_local(&self, imp: &ImplSig) -> bool {
        if self.res.symbol(imp.trait_ref.trait_sym).module == self.module {
            return true;
        }
        for t in std::iter::once(&imp.self_ty).chain(&imp.trait_ref.args) {
            if self.declared_here(t) {
                return true;
            }
            let uncovered = match t {
                Ty::Param(_) => true,
                Ty::Ptr { inner, .. } | Ty::OwnPtr { inner, .. } | Ty::Ref { inner, .. } => {
                    matches!(**inner, Ty::Param(_))
                }
                _ => false,
            };
            if uncovered {
                return false;
            }
        }
        false
    }

    /// Whether `t` is a type this module declares. An imported header's types count as
    /// declared by the module whose manifest carries its recipe, which is one module in any
    /// build, since two recipes for one library are an error (spec §15.1).
    fn declared_here(&self, t: &Ty) -> bool {
        let Ty::Named { sym, .. } = t else { return false };
        let home = self.res.symbol(*sym).module;
        home == self.module || self.ws.recipe_owner(home) == Some(self.module)
    }

    /// Pass 2: the implementation's method signatures, with `Self` replaced by the type.
    pub(crate) fn impl_methods(&mut self, item: &Item, i: &ImplDecl) {
        let Some(&idx) = self.impl_at.get(&(self.file, item.span.start)) else { return };
        let self_ty = self.table.impls[idx].self_ty.clone();
        self.push_impl_scope(idx);
        let empty = HashMap::new();
        for m in &i.items {
            match &m.kind {
                MemberKind::Fn(f) => {
                    // A method of an implementation is as visible as the trait it belongs to.
                    let mut s = self.fn_sig(f, Vis::Public, None);
                    for p in &mut s.params {
                        p.ty = ty::substitute_with(&p.ty, &empty, Some(&self_ty));
                    }
                    s.ret = ty::substitute_with(&s.ret, &empty, Some(&self_ty));
                    let name = f.name.name.clone();
                    if self.table.impls[idx].methods.contains_key(&name) {
                        self.error_help(
                            codes::DUPLICATE_METHOD,
                            f.name.span,
                            format!("`{name}` is implemented twice"),
                            "declared again here",
                            "an implementation has one method per trait method (spec §7.6)",
                        );
                        continue;
                    }
                    let imp = &mut self.table.impls[idx];
                    imp.method_order.push(name.clone());
                    imp.methods.insert(name, s);
                }
                MemberKind::Field(f) => self.error_help(
                    codes::TRAIT_MEMBER_MISMATCH,
                    f.name.span,
                    format!("`{}` is a field, and an implementation has none", f.name.name),
                    "not allowed in an `impl`",
                    "an `impl` holds the trait's methods and its associated-type choices; \
                     fields belong to the struct (spec §7.6)",
                ),
                MemberKind::Tie(t) => self.error_help(
                    codes::TRAIT_MEMBER_MISMATCH,
                    t.span,
                    "a tie is declared by the struct whose memory it names",
                    "not allowed in an `impl`",
                    "write `tie` among the struct's members, beside the fields it names \
                     (spec §6.7)",
                ),
                // A constructor was reported by the parser; an associated type was
                // collected with the header.
                _ => {}
            }
        }
        self.pop_params();
    }

    /// The implementation's parameters and its associated-type choices, in scope for its
    /// members, so `fn get(&self) -> Item` resolves to whatever it chose (spec §8.5).
    pub(crate) fn push_impl_scope(&mut self, idx: usize) {
        let imp = &self.table.impls[idx];
        let mut scope: HashMap<String, Ty> = imp
            .generics
            .iter()
            .map(|d| (d.name().to_owned(), Ty::Param(d.param.clone())))
            .collect();
        for a in &imp.assoc_types {
            if let Some(t) = &a.ty {
                scope.insert(a.name.clone(), t.clone());
            }
        }
        self.param_scopes.push(scope);
    }

    /// `impl Drop` and `impl Copy` become their struct's destructor and copy constructor
    /// (spec §6.1). Recorded against the struct here, after every signature exists and
    /// before any body is checked, because everything about ownership reads them.
    ///
    /// Only an implementation that may be special is attached: the struct's own module, and
    /// the struct itself rather than one instance of it. One that breaks those rules is
    /// reported with the rest of the implementation's checks, in pass 3.
    ///
    /// Only the implementations from `from` on: the ones before are an earlier layer's,
    /// attached already.
    pub(crate) fn attach_special_impls(&mut self, from: usize) {
        let (Some(drop), Some(copy)) = (self.prelude_trait("Drop"), self.prelude_trait("Copy"))
        else {
            return;
        };
        let saved = (self.file, self.module);
        for idx in from..self.table.impls.len() {
            let imp = self.table.impls[idx].clone();
            let is_drop = imp.trait_ref.trait_sym == drop;
            if !is_drop && imp.trait_ref.trait_sym != copy {
                continue;
            }
            let Some(sym) = self.special_owner(&imp) else { continue };
            let method = if is_drop { "drop" } else { "copy" };
            let Some(sig) = imp.methods.get(method).cloned() else { continue };
            (self.file, self.module) = (imp.file, imp.module);
            let taken = match self.table.sigs.get(&sym) {
                Some(ItemSig::Struct(s)) => {
                    if is_drop {
                        s.drop.is_some()
                    } else {
                        s.copy.is_some()
                    }
                }
                _ => continue,
            };
            if taken {
                self.duplicate_special(imp.span, method);
                continue;
            }
            if let Some(ItemSig::Struct(s)) = self.table.sigs.get_mut(&sym) {
                if is_drop {
                    s.drop = Some(sig);
                } else {
                    s.copy = Some(sig);
                }
            }
            if is_drop {
                self.table.drop_impls.insert(sym, idx);
            } else {
                self.table.copy_impls.insert(sym, idx);
            }
        }
        (self.file, self.module) = saved;
    }

    /// The struct an `impl Drop`/`impl Copy` may attach to: a struct, in the struct's own
    /// module, applied to distinct parameters of the implementation — so it covers every
    /// instance rather than one (spec §6.1).
    pub(crate) fn special_owner(&self, imp: &ImplSig) -> Option<SymbolId> {
        let Ty::Named { sym, args } = &imp.self_ty else { return None };
        // Asked before struct signatures exist, so by the declaration's kind.
        if self.res.symbol(*sym).kind != dz_resolve::SymbolKind::Struct {
            return None;
        }
        if self.res.symbol(*sym).module != imp.module {
            return None;
        }
        let mut seen: Vec<u32> = Vec::new();
        for a in args {
            let Ty::Param(p) = a else { return None };
            if seen.contains(&p.id) || !imp.generics.iter().any(|g| g.param.id == p.id) {
                return None;
            }
            seen.push(p.id);
        }
        Some(*sym)
    }
}

// ---------------------------------------------------------------------------
// queries
// ---------------------------------------------------------------------------

impl TypeTable {
    /// Every implementation, addressed by position (spec §7.6).
    pub fn impls(&self) -> &[ImplSig] {
        &self.impls
    }

    /// One of the prelude's traits, by name (spec §3.2).
    pub fn prelude_trait_named(&self, name: &str) -> Option<SymbolId> {
        self.prelude_names.iter().find(|(_, n)| n.as_str() == name).map(|(s, _)| *s)
    }

    /// The struct's `impl Drop`, if it has one (spec §6.1).
    pub fn drop_impl(&self, sym: SymbolId) -> Option<usize> {
        self.drop_impls.get(&sym).copied()
    }

    /// The struct's `impl Copy`, if it has one (spec §6.1).
    pub fn copy_impl(&self, sym: SymbolId) -> Option<usize> {
        self.copy_impls.get(&sym).copied()
    }

    /// An implementation that would give `ty` a method `method` but for a bound its
    /// arguments do not meet — `impl<T: Ord> Sort for List<T>` asked about `List<Plain>` —
    /// as the trait, the argument, and the bound it lacks. So "no method `sort`" can say why.
    pub fn unmet_bound_for(&self, ty: &Ty, method: &str) -> Option<(SymbolId, Ty, TraitRef)> {
        for imp in self.impls() {
            let provides = imp.methods.contains_key(method)
                || self
                    .struct_sig(imp.trait_ref.trait_sym)
                    .and_then(|s| s.methods.get(method))
                    .is_some_and(|m| m.has_body);
            if !provides {
                continue;
            }
            let params: Vec<u32> = imp.generics.iter().map(|g| g.param.id).collect();
            let mut map = HashMap::new();
            if !match_ty(&imp.self_ty, ty, &params, &mut map) {
                continue;
            }
            for g in &imp.generics {
                let Some(arg) = map.get(&g.param.id).cloned() else { continue };
                for b in &g.bounds {
                    let wanted = subst_ref(b, &map, ty);
                    if !self.implements(&arg, &wanted) {
                        return Some((imp.trait_ref.trait_sym, arg, wanted));
                    }
                }
            }
        }
        None
    }

    /// An implementation giving another instance of `ty`'s generic type a method `method`
    /// — `impl<V> StrKeys<V> for HashMap<String, V>` asked about `HashMap<i32, i32>` — as
    /// the trait and the type it is implemented for. So "no method `get_str`" can say which
    /// instances have one.
    pub fn impl_for_other_instance(&self, ty: &Ty, method: &str) -> Option<(SymbolId, Ty)> {
        let Ty::Named { sym, .. } = ty else { return None };
        self.impls().iter().find_map(|imp| {
            let same_type = matches!(&imp.self_ty, Ty::Named { sym: s, .. } if s == sym);
            let provides = imp.methods.contains_key(method)
                || self
                    .struct_sig(imp.trait_ref.trait_sym)
                    .and_then(|s| s.methods.get(method))
                    .is_some_and(|m| m.has_body);
            (same_type && provides).then(|| (imp.trait_ref.trait_sym, imp.self_ty.clone()))
        })
    }

    /// Does `imp` apply to `ty`, and with what for its parameters? `impl<T: Copy> Copy for
    /// Buffer<T>` applies to `Buffer<f32>` with `T = f32`, and not to `Buffer<File>`,
    /// whose argument fails the bound.
    pub fn impl_applies(&self, imp: &ImplSig, ty: &Ty) -> Option<HashMap<u32, Ty>> {
        // Static text is text: every implementation for `str` is one for `const str` (§5.3).
        let ty = if *ty == Ty::ConstStr { &Ty::Str } else { ty };
        let params: Vec<u32> = imp.generics.iter().map(|g| g.param.id).collect();
        let mut map = HashMap::new();
        if !match_ty(&imp.self_ty, ty, &params, &mut map) {
            return None;
        }
        for g in &imp.generics {
            let Some(arg) = map.get(&g.param.id).cloned() else { continue };
            for b in &g.bounds {
                if !self.implements(&arg, &subst_ref(b, &map, ty)) {
                    return None;
                }
            }
        }
        Some(map)
    }

    /// Every implementation that applies to `ty`, with the trait as seen from `ty`.
    ///
    /// A base's implementations are the derived type's too — a `Button` is `Inspect` if
    /// `UiElement` is — and a more specific `impl Inspect for Button` replaces the
    /// inherited one, which is why the chain is walked nearest-first and a trait already
    /// answered for is not answered again (spec §7.6, §7.3, 18.32).
    ///
    /// `Drop` and `Copy` are deliberately **not** inherited here. They are special
    /// members rather than ordinary traits (spec §6.1): C++ already runs a base's
    /// destructor as part of the derived one, and reporting the base's `impl Drop` as the
    /// derived type's would emit a second destructor that ran the same body twice.
    pub fn impls_for(&self, ty: &Ty) -> Vec<(usize, TraitRef, HashMap<u32, Ty>)> {
        let direct: Vec<(usize, TraitRef, HashMap<u32, Ty>)> = self
            .impls
            .iter()
            .enumerate()
            .filter_map(|(i, imp)| {
                let map = self.impl_applies(imp, ty)?;
                Some((i, subst_ref(&imp.trait_ref, &map, ty), map))
            })
            .collect();

        let Some(sym) = ty.named() else { return direct };
        let chain = self.with_bases(sym);
        if chain.len() < 2 {
            return direct;
        }

        let args = match ty {
            Ty::Named { args, .. } => args.clone(),
            _ => Vec::new(),
        };
        let mut out = direct;
        for base in chain.into_iter().skip(1) {
            // A generic base is the instance this type inherits (T-0035).
            let base_ty = Ty::Named { sym: base, args: self.args_as(sym, &args, base) };
            for (i, imp) in self.impls.iter().enumerate() {
                if Some(imp.trait_ref.trait_sym) == self.drop_trait
                    || Some(imp.trait_ref.trait_sym) == self.copy_trait
                {
                    continue;
                }
                let Some(map) = self.impl_applies(imp, &base_ty) else { continue };
                let r = subst_ref(&imp.trait_ref, &map, &base_ty);
                // A nearer implementation of the same trait at the same arguments has
                // already been found, and it is the one that applies.
                if out.iter().any(|(_, x, _)| x.trait_sym == r.trait_sym && x.args == r.args) {
                    continue;
                }
                out.push((i, r, map));
            }
        }
        out
    }

    /// The traits `ty` implements through an `impl`.
    pub fn traits_of(&self, ty: &Ty) -> Vec<TraitRef> {
        self.impls_for(ty).into_iter().map(|(_, r, _)| r).collect()
    }

    /// The implementation of exactly `r` for `ty`.
    pub fn impl_of(&self, r: &TraitRef, ty: &Ty) -> Option<(usize, HashMap<u32, Ty>)> {
        self.impls_for(ty)
            .into_iter()
            .find(|(_, x, _)| x.trait_sym == r.trait_sym && x.args == r.args)
            .map(|(i, _, m)| (i, m))
    }

    /// A prelude trait `t` has without an `impl`: the machine's own types' operator traits
    /// (spec §11.2), and `Eq`, `Hash` and `Ord` for an enum, which is a named integer
    /// (spec §7.4, 18.54).
    pub fn builtin_implements(&self, t: &Ty, trait_name: &str, args: &[Ty]) -> bool {
        primitive_implements(t, trait_name, args)
            || (matches!(trait_name, "Eq" | "Hash" | "Ord") && self.is_enum(t))
    }

    /// Whether `t` is an enum (not a union: a union's payload is not an integer).
    pub fn is_enum(&self, t: &Ty) -> bool {
        matches!(t, Ty::Named { sym, .. } if matches!(self.get(*sym), Some(ItemSig::Enum { .. })))
    }

    /// Does `t` satisfy `r` — the question every bound asks (spec §8.1)?
    pub fn implements(&self, t: &Ty, r: &TraitRef) -> bool {
        // `Copy` is structural: every copyable type has it, declared or not (spec §8.2).
        if Some(r.trait_sym) == self.copy_trait {
            return self.copyable(t);
        }
        // A trait satisfies itself, which is what `^Trait` will need.
        if t.named() == Some(r.trait_sym) {
            return true;
        }
        // The machine's own types implement the prelude's operator traits (spec §11.2).
        if let Some(name) = self.prelude_names.get(&r.trait_sym) {
            if self.builtin_implements(t, name, &r.args) {
                return true;
            }
        }
        let direct = if ty::is_abstract(t) { self.abstract_bounds(t) } else { self.traits_of(t) };
        self.trait_closure(&direct, t)
            .iter()
            .any(|x| x.trait_sym == r.trait_sym && x.args == r.args)
    }

    /// Does `t` implement `r` itself, rather than by way of a trait that requires it? What
    /// `impl Ord for S` asks of `Eq`: that `S` has an `impl Eq` of its own (spec §7.6).
    pub fn implements_directly(&self, t: &Ty, r: &TraitRef) -> bool {
        if Some(r.trait_sym) == self.copy_trait {
            return self.copyable(t);
        }
        if let Some(name) = self.prelude_names.get(&r.trait_sym) {
            if self.builtin_implements(t, name, &r.args) {
                return true;
            }
        }
        if ty::is_abstract(t) {
            self.implements(t, r)
        } else {
            self.impl_of(r, t).is_some()
        }
    }

    /// Which implementation a call of `method` on `recv` reaches (spec §7.6). A type that
    /// implements one trait at several arguments — `Mul<f32>` and `Mul<Vec2>` — has one
    /// implementation per argument type, and the call's first argument picks (§18.27).
    /// Where there is no argument to pick by, as for `ImplicitAs<cstr>` and
    /// `ImplicitAs<str>`, the call's result type `ret` does.
    pub fn resolve_impl(
        &self,
        trait_sym: SymbolId,
        recv: &Ty,
        method: &str,
        arg: Option<&Ty>,
        ret: Option<&Ty>,
    ) -> Option<(usize, HashMap<u32, Ty>)> {
        let found: Vec<(usize, TraitRef, HashMap<u32, Ty>)> =
            self.impls_for(recv).into_iter().filter(|(_, r, _)| r.trait_sym == trait_sym).collect();
        if found.len() > 1 {
            let by = |pick: &dyn Fn(&crate::FnSig) -> bool| {
                found.iter().position(|(_, r, _)| {
                    self.trait_method(r, recv, method).is_some_and(|s| pick(&s))
                })
            };
            let by_arg = arg.and_then(|arg| {
                let arg = arg.peeled().clone();
                by(&|s| s.params.first().is_some_and(|p| *p.ty.peeled() == arg))
            });
            let hit = by_arg.or_else(|| ret.and_then(|ret| by(&|s| s.ret == *ret)));
            if let Some(i) = hit {
                let (idx, _, map) = found.into_iter().nth(i)?;
                return Some((idx, map));
            }
        }
        found.into_iter().next().map(|(i, _, m)| (i, m))
    }
}

/// A trait reference seen through an implementation's parameters and its type.
pub(crate) fn subst_ref(r: &TraitRef, map: &HashMap<u32, Ty>, self_ty: &Ty) -> TraitRef {
    TraitRef {
        trait_sym: r.trait_sym,
        args: r.args.iter().map(|a| ty::substitute_with(a, map, Some(self_ty))).collect(),
        span: r.span,
    }
}

/// Match `pat`, in which the `params` are holes, against `ty`, filling `map`. A hole seen
/// twice must match the same type both times.
fn match_ty(pat: &Ty, ty: &Ty, params: &[u32], map: &mut HashMap<u32, Ty>) -> bool {
    match (pat, ty) {
        (Ty::Param(p), _) if params.contains(&p.id) => match map.get(&p.id) {
            Some(bound) => bound == ty,
            None => {
                map.insert(p.id, ty.clone());
                true
            }
        },
        (Ty::Named { sym: a, args: aa }, Ty::Named { sym: b, args: ba }) => {
            a == b
                && aa.len() == ba.len()
                && aa.iter().zip(ba).all(|(x, y)| match_ty(x, y, params, map))
        }
        (Ty::Optional(a), Ty::Optional(b)) => match_ty(a, b, params, map),
        (Ty::Ptr { mutable: ma, inner: a }, Ty::Ptr { mutable: mb, inner: b })
        | (Ty::OwnPtr { mutable: ma, inner: a }, Ty::OwnPtr { mutable: mb, inner: b }) => {
            ma == mb && match_ty(a, b, params, map)
        }
        (Ty::Ref { mutable: ma, inner: a }, Ty::Ref { mutable: mb, inner: b }) => {
            ma == mb && match_ty(a, b, params, map)
        }
        (Ty::Array { len: la, elem: a }, Ty::Array { len: lb, elem: b }) => {
            la == lb && match_ty(a, b, params, map)
        }
        (Ty::Slice { mutable: ma, elem: a }, Ty::Slice { mutable: mb, elem: b }) => {
            ma == mb && match_ty(a, b, params, map)
        }
        (Ty::Tuple(a), Ty::Tuple(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(x, y)| match_ty(x, y, params, map))
        }
        _ => pat == ty,
    }
}

/// Could one type match both patterns? Every parameter is a hole on either side, so this
/// is the conservative question overlap needs: two implementations of one trait that might
/// both apply to some type are ambiguous, whichever type it turns out to be (spec §7.6).
pub(crate) fn might_overlap(a: &Ty, b: &Ty) -> bool {
    let o = |x: &Ty, y: &Ty| might_overlap(x, y);
    match (a, b) {
        (Ty::Param(_), _) | (_, Ty::Param(_)) => true,
        (Ty::Named { sym: sa, args: aa }, Ty::Named { sym: sb, args: ab }) => {
            sa == sb && aa.len() == ab.len() && aa.iter().zip(ab).all(|(x, y)| o(x, y))
        }
        (Ty::Optional(x), Ty::Optional(y)) => o(x, y),
        (Ty::Ptr { mutable: ma, inner: x }, Ty::Ptr { mutable: mb, inner: y })
        | (Ty::OwnPtr { mutable: ma, inner: x }, Ty::OwnPtr { mutable: mb, inner: y }) => {
            ma == mb && o(x, y)
        }
        (Ty::Ref { mutable: ma, inner: x }, Ty::Ref { mutable: mb, inner: y }) => {
            ma == mb && o(x, y)
        }
        (Ty::Array { len: la, elem: x }, Ty::Array { len: lb, elem: y }) => la == lb && o(x, y),
        (Ty::Slice { mutable: ma, elem: x }, Ty::Slice { mutable: mb, elem: y }) => {
            ma == mb && o(x, y)
        }
        (Ty::Tuple(x), Ty::Tuple(y)) => x.len() == y.len() && x.iter().zip(y).all(|(p, q)| o(p, q)),
        _ => a == b,
    }
}

/// Which of the prelude's operator traits a primitive implements (spec §11.2).
///
/// The operators themselves are built in for these types — that never changed — so this
/// only says what a *bound* may be satisfied by. An arithmetic trait is implemented with
/// `Rhs = Self` only, since there are no implicit numeric conversions (§5.1).
pub(crate) fn primitive_implements(t: &Ty, trait_name: &str, args: &[Ty]) -> bool {
    let numeric = matches!(t, Ty::Int(_) | Ty::Float(_));
    let rhs_is_self = args.first().map_or(true, |a| a == t);
    match trait_name {
        "Eq" => numeric || matches!(t, Ty::Bool | Ty::Char | Ty::Str | Ty::ConstStr),
        // Not floats: `NaN != NaN` and `0.0 == -0.0` break the promise that equal keys hash
        // equally, and a float key is almost always a mistake anyway (spec §14.2).
        "Hash" => matches!(t, Ty::Int(_) | Ty::Bool | Ty::Char | Ty::Str | Ty::ConstStr),
        // `std::math`'s, looked up by symbol so a user trait named `Float` is not these.
        "Float" => matches!(t, Ty::Float(_)),
        "Integer" => matches!(t, Ty::Int(_)),
        "Number" => numeric,
        "Ord" => numeric || matches!(t, Ty::Char),
        "Add" | "Sub" | "Mul" | "Div" => numeric && rhs_is_self,
        "Rem" => matches!(t, Ty::Int(_)) && rhs_is_self,
        "Neg" => matches!(t, Ty::Float(_)) || matches!(t, Ty::Int(i) if i.is_signed()),
        _ => false,
    }
}
