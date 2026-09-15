//! Checking function bodies: statements, expressions, and the rules of spec §9–§11.

use std::collections::HashMap;
use std::rc::Rc;

use dz_ast::*;
use dz_diag::{codes, Diagnostic};
use dz_hir::{self as hir, BlockId, BodyOwner, LocalId, LocalKind, StmtId};
use dz_resolve::{SymbolId, SymbolKind};
use dz_span::Span;

use crate::build::{BodyBuilder, Checked, CheckedBlock};
use crate::impls::might_overlap;
use crate::ty::{self, assignable, join, IntTy, Ty};
use crate::{
    const_int_literal, decode_raw_string_literal, decode_string_literal, parse_float_literal,
    parse_int_literal, type_name, Ctx, FnSig, ImplSig, ItemSig, ParamDef, TraitRef,
};

pub(crate) fn item(cx: &mut Ctx, it: &Item) {
    match &it.kind {
        // Removed by the driver before this phase (spec §12.10).
        ItemKind::CfgIf(_) => {}
        // A generic body is checked **once**, here, against its bounds and nothing else —
        // not once per instantiation (spec §8.1, §8.6). Its parameters go into scope first
        // so that `T` inside it is `Ty::Param` rather than a hole.
        ItemKind::Fn(f) => {
            let sig = cx.symbol_of(it).and_then(|s| match cx.table.get(s) {
                Some(ItemSig::Fn(sig)) => Some(sig.clone()),
                _ => None,
            });
            if let (Some(sig), Some(sym)) = (sig, cx.symbol_of(it)) {
                let native = f.is_extern;
                if f.body.is_none() && !native {
                    cx.error_help(
                        codes::MISSING_BODY,
                        f.name.span,
                        format!("`{}` has no body", f.name.name),
                        "a function needs a body",
                        "only a trait's method or a `virtual` one may leave its body out (spec §7.2)",
                    );
                } else if native && (f.body.is_some() || !sig.generics.is_empty() || sig.fallible) {
                    cx.error_help(
                        codes::MISSING_BODY,
                        f.name.span,
                        format!("`{}` cannot be `extern`", f.name.name),
                        "an `extern fn` is a plain declaration",
                        "an `extern fn` has no body, no type parameters and is not \
                         fallible; its body is the compiler's C++ (spec §7.2)",
                    );
                }
                cx.push_params(&sig.generics);
                let mut body =
                    Body::new(cx, None, sig.ret.clone(), BodyOwner::Fn(sym), f.name.span);
                let root = body.function(f, &sig);
                body.finish(root);
                cx.pop_params();
            }
        }
        ItemKind::Struct(s) => {
            let Some(sym) = cx.symbol_of(it) else { return };
            cx.push_declared_params(sym);
            check_members(cx, sym, &s.members);
            report_duplicate_methods(cx, sym);
            check_overrides(cx, sym);
            cx.pop_params();
        }
        ItemKind::Impl(i) => check_impl(cx, it, i),
        ItemKind::Enum(e) => check_enum(cx, it, e),
        ItemKind::Const(c) => {
            let Some(sym) = cx.symbol_of(it) else { return };
            let declared = match cx.table.get(sym) {
                Some(ItemSig::Const(t)) => t.clone(),
                _ => Ty::Error,
            };
            let mut body = Body::new(cx, None, Ty::Void, BodyOwner::Const(sym), c.value.span);
            let checked = body.expr(&c.value, Some(&declared));
            if !declared.is_error() {
                body.expect(&declared, &checked.ty, c.value.span);
            }
            body.finish_value(c.value.span, checked);
        }
        ItemKind::Static(s) => {
            let Some(sym) = cx.symbol_of(it) else { return };
            let declared = match cx.table.get(sym) {
                Some(ItemSig::Static(t)) => t.clone(),
                _ => Ty::Error,
            };
            let mut body = Body::new(cx, None, Ty::Void, BodyOwner::Static(sym), s.value.span);
            let checked = body.expr(&s.value, Some(&declared));
            if !declared.is_error() {
                body.expect(&declared, &checked.ty, s.value.span);
                // Built before `main`, in an order C++ does not promise across files, so
                // nothing in it may run code that could read another static (spec §7.7).
                if !body.is_static_initialiser(checked.id) {
                    body.cx.error_help(
                        codes::STATIC_INITIALISER,
                        s.value.span,
                        format!("`{}` must be built from constants", s.name.name),
                        "computed when the program starts",
                        "a `static` starts as a literal, a `const`, `null`, `none`, or a struct, \
                         array or variant literal of those; fill it in from `main` if it needs \
                         more (spec §7.7)",
                    );
                }
            }
            body.finish_value(s.value.span, checked);
        }
        // A trait's default methods are checked once, here, with `Self` standing for
        // whichever type inherits them (spec §7.6, §8.1). They used to be skipped outright,
        // so a mistake in one surfaced only when an implementer's copy reached C++.
        // Implementers get a concrete copy at monomorphization; the trait is never emitted.
        ItemKind::Trait(t) => {
            let Some(sym) = cx.symbol_of(it) else { return };
            cx.push_declared_params(sym);
            let keys = cx.table.struct_sig(sym).map(|s| s.method_keys.clone()).unwrap_or_default();
            let fns = t.items.iter().filter_map(|m| match &m.kind {
                MemberKind::Fn(f) => Some(f),
                _ => None,
            });
            for (f, key) in fns.zip(keys) {
                refuse_generic_method(
                    cx,
                    f,
                    "a trait's method is called through implementations the caller may not know, \
                     and through `^Trait` with no type at all; make the trait generic instead, \
                     `trait Show<T>` (spec §7.6, §8.1)",
                );
                if f.body.is_none() {
                    // Required of every implementer. Its default arguments are still the
                    // trait's, and a trait object's handle declares them (spec §7.6).
                    if f.params.iter().any(|p| p.default.is_some()) {
                        let sig =
                            cx.table.struct_sig(sym).and_then(|s| s.methods.get(&key)).cloned();
                        if let Some(sig) = sig {
                            let owner = BodyOwner::TraitDefaults { owner: sym, name: key };
                            cx.push_params(&sig.generics);
                            let mut body =
                                Body::new(cx, Some(sym), sig.ret.clone(), owner, f.name.span);
                            let root = body.function(f, &sig);
                            body.finish(root);
                            cx.pop_params();
                        }
                    }
                    continue;
                }
                let sig = cx.table.struct_sig(sym).and_then(|s| s.methods.get(&key)).cloned();
                if let Some(sig) = sig {
                    let owner = BodyOwner::Method { owner: sym, name: key };
                    cx.push_params(&sig.generics);
                    let mut body = Body::new(cx, Some(sym), sig.ret.clone(), owner, f.name.span);
                    let root = body.function(f, &sig);
                    body.finish(root);
                    cx.pop_params();
                }
            }
            report_duplicate_methods(cx, sym);
            cx.pop_params();
        }
        // Union payload types are checked during signature collection; macros belong to
        // M10.
        ItemKind::Union(_)
        | ItemKind::Macro(_)
        | ItemKind::MacroInvocation { .. }
        | ItemKind::Error => {}
    }
}

/// A method with type parameters of its own where it cannot have them (DZ0557). Reported at
/// its first parameter, once; the method is otherwise checked as written.
fn refuse_generic_method(cx: &mut Ctx, f: &FnDecl, why: &str) {
    let Some(first) = f.generics.params.first() else { return };
    cx.error_help(
        codes::GENERIC_METHOD_NOT_ALLOWED,
        first.name.span,
        format!("`{}` cannot have type parameters of its own", f.name.name),
        "a type parameter of the method",
        why,
    );
}

/// An implementation, `impl Trait for Type` (spec §7.6): where it may be written, whether it
/// overlaps another, whether it implements what its trait asks, and its method bodies.
///
/// Checked where the implementation is written, so a missing method or a mismatched
/// signature is reported at the `impl`, never at some later use.
fn check_impl(cx: &mut Ctx, it: &Item, i: &ImplDecl) {
    let Some(&idx) = cx.impl_at.get(&(cx.file, it.span.start)) else { return };
    let imp = cx.table.impls()[idx].clone();
    let trait_sym = imp.trait_ref.trait_sym;
    let trait_name = cx.res.symbol(trait_sym).name.clone();
    let me = cx.name_of(&imp.self_ty);
    let drop_trait = cx.prelude_trait("Drop");
    let copy_trait = cx.prelude_trait("Copy");
    let special = Some(trait_sym) == drop_trait || Some(trait_sym) == copy_trait;

    // One implementation of a trait per type and arguments, or a call could not choose.
    // Reported at the later one. `Drop` and `Copy` have their own "declared twice".
    if !special {
        for j in 0..idx {
            let other = &cx.table.impls()[j];
            let same_args = other.trait_ref.args.len() == imp.trait_ref.args.len()
                && other
                    .trait_ref
                    .args
                    .iter()
                    .zip(&imp.trait_ref.args)
                    .all(|(a, b)| might_overlap(a, b));
            if other.trait_ref.trait_sym == trait_sym
                && same_args
                && might_overlap(&other.self_ty, &imp.self_ty)
            {
                let (ofile, ospan) = (other.file, other.span);
                cx.diags.push(
                    Diagnostic::error(
                        codes::OVERLAPPING_IMPL,
                        format!("`{trait_name}` is implemented twice for `{me}`"),
                    )
                    .at(cx.file, imp.span, "a second implementation")
                    .with_label(dz_diag::Label::secondary(ofile, ospan, "the first"))
                    .with_help(
                        "a type has one implementation of a trait for each set of arguments, \
                         or a call could not know which to use (spec §7.6)",
                    ),
                );
                break;
            }
        }
    }

    check_impl_conformance(cx, &imp, &trait_name, &me);

    // `impl Ord for S` promises an `Eq` as well, since every `Ord` is an `Eq` (spec §7.6).
    let map = imp.trait_ref.subst_map(cx.table.type_params(trait_sym));
    for s in cx.table.supertraits(trait_sym).to_vec() {
        let s = TraitRef {
            trait_sym: s.trait_sym,
            args: s.args.iter().map(|a| ty::substitute_with(a, &map, Some(&imp.self_ty))).collect(),
            span: s.span,
        };
        if !cx.table.implements_directly(&imp.self_ty, &s) {
            let sup = cx.res.symbol(s.trait_sym).name.clone();
            cx.error_help(
                codes::UNSATISFIED_BOUND,
                imp.span,
                format!("`{me}` implements `{trait_name}` but not `{sup}`, which it requires"),
                format!("`{trait_name}` requires `{sup}`"),
                format!("add `impl {sup} for {me} {{ … }}` (spec §7.6)"),
            );
        }
    }

    // The bodies. A method sees the type's private members only when the implementation
    // is in the type's own module (spec §4, §7.6).
    let privileged = match &imp.self_ty {
        Ty::Named { sym, .. } if cx.res.symbol(*sym).module == cx.module => Some(*sym),
        _ => None,
    };
    let special_owner = if special { cx.special_owner(&imp) } else { None };
    cx.push_impl_scope(idx);
    for m in &i.items {
        let MemberKind::Fn(f) = &m.kind else { continue };
        let Some(sig) = imp.methods.get(&f.name.name).cloned() else { continue };
        if f.body.is_none() {
            cx.error_help(
                codes::TRAIT_MEMBER_MISMATCH,
                f.name.span,
                format!("`{}` has no body", f.name.name),
                "an implementation must supply it",
                "only the trait's own declaration may leave a method without a body (spec §7.6)",
            );
            continue;
        }
        // `Drop` and `Copy` are the struct's destructor and copy constructor, and are
        // owned as such, so ownership analysis and codegen find them where they always did.
        let owner = match special_owner {
            Some(sym) if Some(trait_sym) == drop_trait && f.name.name == "drop" => {
                BodyOwner::Drop(sym)
            }
            Some(sym) if Some(trait_sym) == copy_trait && f.name.name == "copy" => {
                BodyOwner::Copy(sym)
            }
            _ => BodyOwner::ImplMethod { imp: idx as u32, name: f.name.name.clone() },
        };
        cx.push_params(&sig.generics);
        let mut body = Body::new(cx, privileged, sig.ret.clone(), owner, f.name.span);
        body.self_full = Some(imp.self_ty.clone());
        let root = body.function(f, &sig);
        body.finish(root);
        cx.pop_params();
    }
    cx.pop_params();
}

/// Does an implementation supply what its trait asks (spec §7.6, §8.5)? Every method the
/// trait gives no default for, with the declared signature; nothing the trait does not
/// declare; and every associated type the trait leaves to the implementer.
fn check_impl_conformance(cx: &mut Ctx, imp: &ImplSig, trait_name: &str, me: &str) {
    let r = &imp.trait_ref;
    let Some(spec) = cx.table.struct_sig(r.trait_sym).cloned() else { return };
    for method in &spec.method_keys {
        // Read through this implementation, so `Mul<f32>` asks for `fn mul(&self, rhs:
        // f32)` rather than for `Rhs` (spec §11.2).
        let Some(want) = cx.table.trait_method(r, &imp.self_ty, method) else { continue };
        match imp.methods.get(method) {
            None if spec.methods[method].has_body => {}
            None => {
                let shown = signature_of(cx, method, &want);
                // The signature is already known exactly, so the diagnostic carries the
                // stub as an edit (spec §17.4). The body panics rather than returning a
                // default: a stub that silently answers `0` is a bug waiting to be found
                // at run time, and one that refuses to compile is a note to finish it.
                // A method with a return type therefore still reports `DZ0405` until it
                // is written — `panic` is not one of the divergent forms that satisfy
                // definite return (§10.5).
                let stub = vec![
                    format!("{shown} {{"),
                    "    panic(\"not implemented\")".to_owned(),
                    "}".to_owned(),
                ];
                let mut d = Diagnostic::error(
                    codes::MISSING_TRAIT_MEMBER,
                    format!("`impl {trait_name} for {me}` does not implement `{method}`"),
                )
                .at(cx.file, imp.span, format!("`{trait_name}` requires `{method}`"))
                .with_help(format!("add `{shown}` (spec §7.6)"));
                if let Some(edit) = cx.insert_inside_braces(imp.block_span, &stub) {
                    d = d.with_fix(format!("implement `{method}`"), vec![edit]);
                }
                cx.diags.push(d);
            }
            Some(got) => {
                if let Some(why) = signature_differs(cx, &want, got) {
                    let shown = signature_of(cx, method, &want);
                    cx.diags.push(
                        Diagnostic::error(
                            codes::TRAIT_MEMBER_MISMATCH,
                            format!("`{method}` does not match `{trait_name}::{method}`"),
                        )
                        .at(cx.file, got.span, why)
                        .with_help(format!("`{trait_name}` declares `{shown}`")),
                    );
                }
            }
        }
    }
    for name in &imp.method_order {
        if spec.methods.contains_key(name) {
            continue;
        }
        let span = imp.methods[name].span;
        cx.error_help(
            codes::TRAIT_MEMBER_MISMATCH,
            span,
            format!("`{name}` is not a method of `{trait_name}`"),
            "not in the trait",
            "an implementation holds exactly the trait's methods; a method of the type's own \
             goes in the struct (spec §7.6)",
        );
    }
    for assoc in &spec.assoc_types {
        if assoc.ty.is_some() {
            continue; // the trait chose it itself
        }
        match imp.assoc_types.iter().find(|a| a.name == assoc.name) {
            None => cx.error_help(
                codes::MISSING_TRAIT_MEMBER,
                imp.span,
                format!("`impl {trait_name} for {me}` does not choose `{}`", assoc.name),
                format!("`{trait_name}` names an associated type `{}`", assoc.name),
                format!(
                    "add `type {} = <some type>` — an associated type is chosen by each \
                     implementation (spec §8.5)",
                    assoc.name
                ),
            ),
            Some(chosen) if chosen.ty.is_none() => cx.error_help(
                codes::MISSING_TRAIT_MEMBER,
                chosen.span,
                format!("`{}` has no type", chosen.name),
                "declared but not chosen",
                format!("write `type {} = <some type>` (spec §8.5)", chosen.name),
            ),
            Some(_) => {}
        }
    }
    for chosen in &imp.assoc_types {
        if !spec.assoc_types.iter().any(|a| a.name == chosen.name) {
            cx.error_help(
                codes::TRAIT_MEMBER_MISMATCH,
                chosen.span,
                format!("`{trait_name}` has no associated type `{}`", chosen.name),
                "not in the trait",
                "an implementation chooses the associated types its trait names (spec §8.5)",
            );
        }
    }
}

/// A struct declares each method once. The only methods that share a name are those of
/// one trait implemented at several arguments, and they live in separate `impl` blocks
/// (spec §7.6, §18.27) — so every repeat here is reported, at the later declaration.
/// `virtual` and `override`, checked in both directions (spec §7.3, 18.32).
///
/// A method that replaces a base's must say `override`, and an `override` must have
/// something to replace — so neither a silent shadow nor a signature that has drifted
/// away from the base's can get through. The signature must match exactly: the whole
/// point of a `virtual` is that a call through a `&Base` reaches this body.
fn check_overrides(cx: &mut Ctx, sym: SymbolId) {
    let Some(sig) = cx.table.struct_sig(sym).cloned() else { return };
    let me = cx.res.symbol(sym).name.clone();
    let bases: Vec<SymbolId> = cx.table.with_bases(sym).into_iter().skip(1).collect();

    for key in &sig.method_keys {
        let Some(mine) = sig.methods.get(key) else { continue };
        let name = key.split('#').next().unwrap_or(key);
        // The nearest base declaring a method of this name is the one being replaced.
        let found = bases.iter().find_map(|b| {
            let bsig = cx.table.struct_sig(*b)?;
            let bkey = bsig.overloads.get(name)?.first()?;
            Some((*b, bsig.methods.get(bkey)?.clone()))
        });

        match (mine.dispatch, found) {
            (Dispatch::Override, None) => {
                cx.error_help(
                    codes::BAD_OVERRIDE,
                    mine.span,
                    format!("`{me}::{name}` overrides nothing"),
                    "no base declares this method",
                    format!(
                        "`override` replaces a base's `virtual fn {name}`; without one to \
                         replace, drop the `override` and this is an ordinary method \
                         (spec §7.3)"
                    ),
                );
            }
            (Dispatch::Static | Dispatch::Virtual, Some((b, theirs))) => {
                let bname = cx.res.symbol(b).name.clone();
                let what = if mine.dispatch == Dispatch::Virtual {
                    format!(
                        "`virtual` declares a new one; to replace `{bname}`'s, write \
                         `override fn {name}`"
                    )
                } else {
                    format!(
                        "a name means one thing in a type — write `override fn {name}` to \
                         replace `{bname}`'s, or rename this one (spec §7.3)"
                    )
                };
                cx.diags.push(
                    dz_diag::Diagnostic::error(
                        codes::SHADOWS_BASE_MEMBER,
                        format!("`{name}` is already inherited from `{bname}`"),
                    )
                    .at(cx.file, mine.span, "redeclared here")
                    .with_label(dz_diag::Label::secondary(
                        cx.res.symbol(b).file,
                        theirs.span,
                        "inherited from here",
                    ))
                    .with_help(what),
                );
            }
            (Dispatch::Override, Some((b, theirs))) => {
                if theirs.dispatch == Dispatch::Static {
                    let bname = cx.res.symbol(b).name.clone();
                    cx.diags.push(
                        dz_diag::Diagnostic::error(
                            codes::BAD_OVERRIDE,
                            format!("`{bname}::{name}` is not `virtual`"),
                        )
                        .at(cx.file, mine.span, "cannot be overridden")
                        .with_label(dz_diag::Label::secondary(
                            cx.res.symbol(b).file,
                            theirs.span,
                            "declared here, without `virtual`",
                        ))
                        .with_help(format!(
                            "a method is dispatched statically unless it says otherwise — \
                             mark `{bname}`'s `virtual fn {name}` to let a derived type \
                             replace it (spec §7.3)"
                        )),
                    );
                } else {
                    check_override_signature(cx, &me, name, mine, b, &theirs);
                }
            }
            _ => {}
        }
    }
}

/// An `override` must have the base's signature exactly. A call through a `&Base` is
/// compiled against the base's declaration, so anything else would be a different method
/// wearing its name (spec §7.3).
fn check_override_signature(
    cx: &mut Ctx,
    me: &str,
    name: &str,
    mine: &FnSig,
    base: SymbolId,
    theirs: &FnSig,
) {
    let bname = cx.res.symbol(base).name.clone();
    let mismatch = if mine.params.len() != theirs.params.len() {
        Some(format!(
            "it takes {} argument(s) where `{bname}::{name}` takes {}",
            mine.params.len(),
            theirs.params.len()
        ))
    } else if let Some(i) =
        (0..mine.params.len()).find(|&i| mine.params[i].ty != theirs.params[i].ty)
    {
        Some(format!(
            "argument {} is `{}` where `{bname}::{name}` has `{}`",
            i + 1,
            cx.name_of(&mine.params[i].ty),
            cx.name_of(&theirs.params[i].ty)
        ))
    } else if mine.ret != theirs.ret {
        Some(format!(
            "it returns `{}` where `{bname}::{name}` returns `{}`",
            cx.name_of(&mine.ret),
            cx.name_of(&theirs.ret)
        ))
    } else if mine.is_mut != theirs.is_mut {
        let want = if theirs.is_mut { "&mut self" } else { "&self" };
        Some(format!("`{bname}::{name}` takes `{want}`"))
    } else if mine.fallible != theirs.fallible {
        let want = if theirs.fallible { "is fallible" } else { "is not fallible" };
        Some(format!("`{bname}::{name}` {want}"))
    } else {
        None
    };

    let Some(detail) = mismatch else { return };
    cx.diags.push(
        dz_diag::Diagnostic::error(
            codes::BAD_OVERRIDE,
            format!("`{me}::{name}` does not match the method it overrides"),
        )
        .at(cx.file, mine.span, detail)
        .with_label(dz_diag::Label::secondary(
            cx.res.symbol(base).file,
            theirs.span,
            "overriding this",
        ))
        .with_help(
            "an override is reached through a `&Base`, which is compiled against the \
             base's signature — so the two must be the same (spec §7.3)"
                .to_owned(),
        ),
    );
}

fn report_duplicate_methods(cx: &mut Ctx, sym: SymbolId) {
    let Some(sig) = cx.table.struct_sig(sym).cloned() else { return };
    let me = cx.res.symbol(sym).name.clone();
    for key in &sig.method_keys {
        if !key.contains('#') {
            continue;
        }
        let base = key.split('#').next().unwrap_or(key);
        let Some(m) = sig.methods.get(key) else { continue };
        cx.error_help(
            codes::DUPLICATE_METHOD,
            m.span,
            format!("`{me}` has more than one method named `{base}`"),
            "declared again here",
            "a struct declares each method once; to implement one trait at several arguments, \
             like `Mul<f32>` and `Mul<Vec2>`, write one `impl` for each (spec §18.27)",
        );
    }
}

/// Why two signatures differ, phrased for a label. `None` when they match.
fn signature_differs(cx: &Ctx, want: &FnSig, got: &FnSig) -> Option<String> {
    if want.params.len() != got.params.len() {
        let (w, g) = (want.params.len(), got.params.len());
        return Some(format!("takes {g} {}, but the trait declares {w}", plural(g, "parameter")));
    }
    for (i, (w, g)) in want.params.iter().zip(&got.params).enumerate() {
        if w.ty != g.ty {
            let (wn, gn) = (type_name(cx.res, &w.ty), type_name(cx.res, &g.ty));
            return Some(format!("parameter {} is `{gn}`, but the trait declares `{wn}`", i + 1));
        }
    }
    if want.ret != got.ret {
        let (wn, gn) = (type_name(cx.res, &want.ret), type_name(cx.res, &got.ret));
        return Some(format!("returns `{gn}`, but the trait declares `{wn}`"));
    }
    if want.fallible != got.fallible {
        return Some(if want.fallible {
            "the trait declares this fallible (`!->`)".to_owned()
        } else {
            "the trait does not declare this fallible".to_owned()
        });
    }
    if want.takes_self != got.takes_self {
        return Some(match want.takes_self {
            Some(true) => "the trait's method takes `&mut self`".to_owned(),
            Some(false) => "the trait's method takes `&self`".to_owned(),
            None => "the trait declares this without a receiver".to_owned(),
        });
    }
    if want.is_unsafe != got.is_unsafe {
        return Some(if want.is_unsafe {
            "the trait declares this `unsafe fn`".to_owned()
        } else {
            "the trait does not declare this `unsafe`".to_owned()
        });
    }
    None
}

/// A signature as it would be written in source, for a `help:` that can be pasted.
fn signature_of(cx: &Ctx, name: &str, sig: &FnSig) -> String {
    let recv = match sig.takes_self {
        Some(true) => vec!["&mut self".to_owned()],
        Some(false) => vec!["&self".to_owned()],
        None => vec![],
    };
    let params: Vec<String> = recv
        .into_iter()
        .chain(sig.params.iter().map(|p| format!("{}: {}", p.name, type_name(cx.res, &p.ty))))
        .collect();
    let arrow = if sig.fallible { "!->" } else { "->" };
    let ret = match &sig.ret {
        Ty::Void => String::new(),
        other => format!(" {arrow} {}", type_name(cx.res, other)),
    };
    let unsafe_ = if sig.is_unsafe { "unsafe " } else { "" };
    format!("{unsafe_}fn {name}({}){ret}", params.join(", "))
}

fn check_members(cx: &mut Ctx, sym: SymbolId, members: &[Member]) {
    check_owns_are_released(cx, sym);
    // Constructors are addressed by position in declaration order (spec §7.2), and methods
    // by key, since two may share a name (spec §18.27).
    let mut ctors = 0usize;
    let keys = cx.table.struct_sig(sym).map(|s| s.method_keys.clone()).unwrap_or_default();
    let mut fns = 0usize;
    for m in members {
        match &m.kind {
            // Removed by the driver before this phase (spec §12.10).
            MemberKind::CfgIf(_) => {}
            MemberKind::Const(c) => {
                let declared = cx
                    .table
                    .struct_sig(sym)
                    .and_then(|s| s.consts.iter().find(|x| x.name == c.name.name))
                    .map(|x| x.ty.clone())
                    .unwrap_or(Ty::Error);
                let owner = BodyOwner::AssocConst { owner: sym, name: c.name.name.clone() };
                let mut body = Body::new(cx, None, Ty::Void, owner, c.value.span);
                let checked = body.expr(&c.value, Some(&declared));
                if !declared.is_error() {
                    body.expect(&declared, &checked.ty, c.value.span);
                    // Folded into every use, so it may not do anything a use would repeat:
                    // constants, and the address of a static — which is what lets a
                    // `thread_local` one be each thread's own (spec §7.3, §7.7).
                    if !body.is_assoc_const_value(checked.id) {
                        body.cx.error_help(
                            codes::STATIC_INITIALISER,
                            c.value.span,
                            format!(
                                "`{}` must be a constant or the address of a static",
                                c.name.name
                            ),
                            "computed when it is used",
                            "a constant in a type is a literal, a `const`, a literal built of \
                             those, or `&STATIC` / `unsafe { &mut STATIC }` (spec §7.3)",
                        );
                    }
                }
                body.finish_value(c.value.span, checked);
            }
            MemberKind::Field(f) => {
                if let Some(default) = &f.default {
                    let found = cx
                        .table
                        .struct_sig(sym)
                        .and_then(|s| s.fields.iter().position(|x| x.name == f.name.name));
                    let expected = found
                        .and_then(|i| cx.table.struct_sig(sym).map(|s| s.fields[i].ty.clone()))
                        .unwrap_or(Ty::Error);
                    let owner = BodyOwner::FieldDefault { owner: sym, index: found.unwrap_or(0) };
                    let mut body = Body::new(cx, Some(sym), Ty::Void, owner, default.span);
                    let checked = body.expr(default, Some(&expected));
                    body.expect(&expected, &checked.ty, default.span);
                    body.finish_value(default.span, checked);
                }
            }
            MemberKind::Fn(f) => {
                let key = keys.get(fns).cloned().unwrap_or_else(|| f.name.name.clone());
                fns += 1;
                if f.dispatch != Dispatch::Static {
                    refuse_generic_method(
                        cx,
                        f,
                        "a `virtual` method has one slot in the vtable, and an instance per type \
                         argument would need one each; take the varying part as a trait \
                         reference instead (spec §7.3, §8.1)",
                    );
                }
                let sig = cx.table.struct_sig(sym).and_then(|s| s.methods.get(&key)).cloned();
                if let Some(sig) = sig {
                    let owner = BodyOwner::Method { owner: sym, name: key };
                    // The method's own parameters on top of the struct's, which the caller
                    // has already pushed (spec §8.1).
                    cx.push_params(&sig.generics);
                    let mut body = Body::new(cx, Some(sym), sig.ret.clone(), owner, f.name.span);
                    let root = body.function(f, &sig);
                    body.finish(root);
                    cx.pop_params();
                }
            }
            MemberKind::Ctor(f) => {
                // A constructor runs on the object, so `self` is mutable.
                let (params, variadic) = cx.param_sigs(&f.params);
                let sig = FnSig {
                    ret_cast: false,
                    dispatch: Dispatch::Static,
                    variadic,
                    params,
                    ret: Ty::Void,
                    fallible: false,
                    takes_self: Some(true),
                    is_mut: true,
                    is_unsafe: false,
                    vis: m.vis,
                    span: f.name.span,
                    generics: Vec::new(),
                    has_body: f.body.is_some(),
                };
                let owner = BodyOwner::Ctor { owner: sym, index: ctors };
                ctors += 1;
                let mut body = Body::new(cx, Some(sym), Ty::Void, owner, f.name.span);
                let root = body.function(f, &sig);
                body.finish(root);
            }
            MemberKind::TypeAlias { .. } | MemberKind::Error => {}
        }
    }
}

/// Spec §6.5, decision §18.4: **a type with an `own` field and no `drop` is an error.**
///
/// `own` is a promise that this struct releases the resource, so a struct that makes it
/// and never keeps it is a leak the compiler can see from the declaration alone. No flow
/// information is involved, which is why the rule lives here rather than in `dz_flow`.
fn check_owns_are_released(cx: &mut Ctx, sym: SymbolId) {
    // Keyed on the lowered type rather than on the AST's `own` flag: `own ^T` reaches
    // the parser two ways — as a field modifier and as part of the type — and only the
    // signature has both already folded into one answer.
    let first = match cx.table.struct_sig(sym) {
        Some(s) if s.drop.is_none() => s
            .fields
            .iter()
            .find(|f| matches!(f.ty, Ty::OwnPtr { .. }))
            .map(|f| (f.name.clone(), f.span)),
        _ => None,
    };
    let Some((name, span)) = first else { return };
    let owner = cx.res.symbol(sym).name.clone();
    cx.error_help(
        codes::OWN_FIELD_WITHOUT_DROP,
        span,
        format!("`{owner}` owns a resource but never releases it"),
        "this field is marked `own`",
        format!(
            "add a `drop()` that releases `{name}`, or remove `own` if {owner} does not \
             own this pointer (spec §6.5)"
        ),
    );
}

fn check_enum(cx: &mut Ctx, it: &Item, e: &EnumDecl) {
    let Some(sym) = cx.symbol_of(it) else { return };
    let backing = match cx.table.get(sym) {
        Some(ItemSig::Enum { backing, .. }) => *backing,
        _ => IntTy::I32,
    };
    let (lo, hi) = backing.range();
    for v in &e.variants {
        let Some(value) = &v.value else { continue };
        let Some(n) = const_int_literal(value) else {
            cx.error_help(
                codes::TYPE_MISMATCH,
                value.span,
                "enum values must be integer constants",
                "not a constant integer",
                "enums are named integers (spec §7.4)",
            );
            continue;
        };
        {
            if n < lo || n > hi {
                let name = backing.name();
                cx.error_help(
                    codes::LITERAL_OUT_OF_RANGE,
                    value.span,
                    format!("`{n}` does not fit in `{name}`"),
                    format!("outside the range of `{name}`"),
                    format!("valid values are {lo} to {hi}"),
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------

#[derive(Clone)]
struct Local {
    ty: Ty,
    mutable: bool,
    slot: Slot,
}

/// What one pass of a `for` hands to its bindings (spec §10.4).
enum ForItem {
    /// A value with no home: a range's number, what a consuming loop moves out, an
    /// iterator's `get` that returns by value. Only `e` can take it.
    Value(Ty),
    /// An element that lives somewhere — in an array, a slice, or behind the reference an
    /// iterator returns. `e` copies it, `&e` refers to it, `&mut e` also writes it.
    Place { ty: Ty, writable: bool },
}

/// What an in-scope name denotes once it is resolved.
///
/// A method body sees its own fields unqualified (spec §7.3), so not every name in scope
/// is a variable. Recording which is which here is what stops the HIR from turning a field
/// into a local that codegen would then emit as an undeclared identifier.
#[derive(Clone, Copy)]
enum Slot {
    Local(LocalId),
    /// A field of `self`, by index into the type's declaration order.
    SelfField {
        index: usize,
    },
}

struct Body<'a, 'c> {
    cx: &'c mut Ctx<'a>,
    scopes: Vec<HashMap<String, Local>>,
    /// The enclosing function's return type, for `return` and `try`.
    ret: Ty,
    self_ty: Option<SymbolId>,
    /// Whether the enclosing signature is `!->`, which is what decides where a failure
    /// can be propagated to (spec §9.5).
    fallible: bool,
    /// Whether the expression being checked is a whole statement. `or ignore` produces
    /// nothing, so it is legal only here — and an expected type is not the signal, since
    /// `let v = f()` and a bare `f()` both arrive with no expectation.
    stmt_position: bool,
    /// For each `if` or `switch` being checked for its value with no type to go by, the
    /// `none` arms still waiting for one: `if alive { 100 } else { none }` learns that the
    /// `none` is an `i32?` only from its sibling (spec §9.1, 18.53).
    pending_nones: Vec<Vec<(hir::ExprId, Span)>>,
    /// The locals `if let` bound, so a write to one is explained as an `if let`'s, not a
    /// loop's (spec §9.4).
    if_let_bindings: Vec<hir::LocalId>,
    /// Inside a constructor's `: Base(…)`, which runs before the constructor has begun,
    /// so there is nothing yet to `return` from (spec §7.3).
    in_base_init: bool,
    self_mutable: bool,
    /// The `self` parameter's local, for expanding unqualified field access.
    self_local: Option<LocalId>,
    /// How many `unsafe` blocks enclose the expression being checked; an `unsafe fn`'s
    /// body starts at one (spec §5.2).
    unsafe_depth: u32,
    /// Set while checking what is indexed, iterated, or asked for its `.len`/`.ptr` — the
    /// places that reach *into* a sequence rather than take it as a value. An imported C
    /// array member may only be used there, since C++ cannot copy one (spec §15.2).
    reaching_into: bool,
    /// Spans `require_unsafe` has reported. One missing `unsafe` around
    /// `construct(m_data[i], v)` is one mistake, not one per operation inside it.
    unsafe_reported: Vec<Span>,
    /// What `Self` and `self` are in an implementation's method: the type implemented for,
    /// which need not be a struct (`impl Inspect for f32`) — spec §7.6. `self_ty` is then
    /// the struct only when the implementation may see its private members.
    self_full: Option<Ty>,
    /// The type `self` was bound with, for an unqualified field's base.
    self_local_ty: Option<Ty>,
    /// The loops enclosing the expression being checked, innermost last, for `break` and
    /// `continue` and the labels they name (spec §10.2).
    loops: Vec<LoopFrame>,
    next_loop: u32,
    hir: BodyBuilder,
}

/// One enclosing loop, while its body is checked.
struct LoopFrame {
    id: u32,
    label: Option<String>,
    /// A `break` or `continue` inside a nested loop names this one, so C++ needs a label.
    breaks: bool,
    continues: bool,
}

impl<'a, 'c> Body<'a, 'c> {
    fn new(
        cx: &'c mut Ctx<'a>,
        self_ty: Option<SymbolId>,
        ret: Ty,
        owner: BodyOwner,
        span: Span,
    ) -> Body<'a, 'c> {
        let file = cx.file;
        let hir = BodyBuilder::new(owner, file, span, ret.clone());
        Body {
            cx,
            scopes: Vec::new(),
            ret,
            self_ty,
            fallible: false,
            stmt_position: false,
            pending_nones: Vec::new(),
            if_let_bindings: Vec::new(),
            in_base_init: false,
            self_mutable: false,
            self_local: None,
            unsafe_depth: 0,
            reaching_into: false,
            unsafe_reported: Vec::new(),
            self_full: None,
            self_local_ty: None,
            loops: Vec::new(),
            next_loop: 0,
            hir,
        }
    }

    /// Spec §5.2: a raw-memory operation is allowed only inside `unsafe`. The compiler
    /// cannot see whether a pointer still points at something, so the programmer says so,
    /// in a place a reader — or `grep` — can find.
    fn require_unsafe(&mut self, span: Span, what: &str) {
        if self.unsafe_depth > 0 {
            return;
        }
        let nested = |a: &Span, b: &Span| a.start <= b.start && b.end <= a.end;
        if self.unsafe_reported.iter().any(|s| nested(s, &span) || nested(&span, s)) {
            return;
        }
        self.unsafe_reported.push(span);
        self.cx.error_help(
            codes::UNSAFE_OPERATION,
            span,
            format!("{what} is unsafe"),
            "outside an `unsafe` block",
            "wrap it in `unsafe { … }` — nothing checks that the memory is still there. \
             To use a pointer safely, turn it into a reference once with `p or …` (spec §5.2)",
        );
    }

    /// Register a body that is one expression: a `const` initialiser or a field default.
    fn finish_value(mut self, span: Span, value: Checked) {
        let stmt = self.hir.stmt(span, hir::StmtKind::Expr(value.id));
        let root = self.hir.block(span, vec![stmt]);
        self.finish(root);
    }

    /// Register the finished body. Called once per body, at the end of checking it.
    fn finish(self, root: BlockId) {
        let fallible = self.fallible;
        let body = self.hir.finish(root, fallible);
        self.cx.hir.push(body);
    }

    // --- scopes -----------------------------------------------------------

    fn push(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop(&mut self) {
        self.scopes.pop();
    }

    fn bind(&mut self, name: &str, ty: Ty, mutable: bool, kind: LocalKind, span: Span) -> LocalId {
        let id = self.hir.local(name, ty.clone(), mutable, span, kind);
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.to_owned(), Local { ty, mutable, slot: Slot::Local(id) });
        }
        id
    }

    /// Bring a field of `self` into scope unqualified (spec §7.3).
    fn bind_self_field(&mut self, name: &str, ty: Ty, mutable: bool, index: usize) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.to_owned(), Local { ty, mutable, slot: Slot::SelfField { index } });
        }
    }

    fn local(&self, name: &str) -> Option<&Local> {
        self.scopes.iter().rev().find_map(|s| s.get(name))
    }

    /// `Self` means the enclosing type.
    /// Replace `Self` with the type whose body this is.
    ///
    /// Total over the type tree, so `Self` is replaced wherever it sits — `[]Self`,
    /// `Buffer<Self>`, `(Self, i32)`. A generic parameter is *not* substituted here: inside
    /// the body that declares it, `T` is a type in its own right (spec §8.1), and replacing
    /// it is monomorphization's job.
    fn subst(&self, t: &Ty) -> Ty {
        let rc = |x: &Rc<Ty>| Rc::new(self.subst(x));
        match t {
            Ty::SelfTy => match (&self.self_full, self.self_ty) {
                (Some(t), _) => t.clone(),
                (None, Some(s)) => self.cx.self_type_of(s),
                (None, None) => Ty::Error,
            },
            Ty::Ptr { mutable, inner } => Ty::Ptr { mutable: *mutable, inner: rc(inner) },
            Ty::OwnPtr { mutable, inner } => Ty::OwnPtr { mutable: *mutable, inner: rc(inner) },
            Ty::Ref { mutable, inner } => Ty::Ref { mutable: *mutable, inner: rc(inner) },
            Ty::Optional(i) => Ty::Optional(rc(i)),
            Ty::Fallible(i) => Ty::Fallible(rc(i)),
            Ty::Range(i) => Ty::Range(rc(i)),
            Ty::Array { len, elem } => Ty::Array { len: *len, elem: rc(elem) },
            Ty::Slice { mutable, elem } => Ty::Slice { mutable: *mutable, elem: rc(elem) },
            Ty::Tuple(parts) => Ty::Tuple(parts.iter().map(|p| self.subst(p)).collect()),
            Ty::Named { sym, args } => {
                Ty::Named { sym: *sym, args: args.iter().map(|a| self.subst(a)).collect() }
            }
            Ty::Fn { params, ret } => {
                Ty::Fn { params: params.iter().map(|p| self.subst(p)).collect(), ret: rc(ret) }
            }
            other => other.clone(),
        }
    }

    /// A `&Derived` where a `&Base` is wanted, and a `^Derived` where a `^Base` is
    /// (spec §7.3, 18.32).
    ///
    /// Free, because the base's fields come first and the C++ output is real single
    /// inheritance — so the two are the same address. Behind a reference or a pointer
    /// only: a *by-value* `Derived` where a `Base` is wanted would have to drop the
    /// derived half, which is C++'s slicing and is an error here. Not through a slice
    /// either, where the element stride would be the derived type's.
    fn base_convertible(&self, expected: &Ty, actual: &Ty) -> bool {
        let (want_mut, want, have_mut, have) = match (expected, actual) {
            (Ty::Ref { mutable: wm, inner: w }, Ty::Ref { mutable: hm, inner: h })
            | (
                Ty::Ptr { mutable: wm, inner: w },
                Ty::Ptr { mutable: hm, inner: h }
                | Ty::OwnPtr { mutable: hm, inner: h }
                | Ty::Ref { mutable: hm, inner: h },
            ) => (*wm, w, *hm, h),
            _ => return false,
        };
        // Writability follows the ordinary rule: a read-only view never gains the right
        // to write (spec §5.2).
        if want_mut && !have_mut {
            return false;
        }
        match (want.as_ref(), have.as_ref()) {
            // `&Square` where a `&Shape` is wanted: a fat pointer to it, when `Square`
            // implements `Shape` (spec §7.6, §18.6). A trait object is not converted to
            // another trait's, even a supertrait's.
            (Ty::Named { sym, args }, have)
                if self.cx.res.symbol(*sym).kind == SymbolKind::Trait
                    && have.named() != Some(*sym) =>
            {
                let wanted = TraitRef { trait_sym: *sym, args: args.clone(), span: Span::DUMMY };
                !have.is_wildcard()
                    && !have
                        .named()
                        .is_some_and(|h| self.cx.res.symbol(h).kind == SymbolKind::Trait)
                    && self.cx.table.implements(have, &wanted)
            }
            (Ty::Named { sym: base, args: ba }, Ty::Named { sym: derived, args: da })
                if ba.is_empty() && da.is_empty() =>
            {
                self.cx.table.inherits_from(*derived, *base)
            }
            _ => false,
        }
    }

    /// `Rc<Button>` where an `Rc<UiElement>` is wanted, and the same for `Weak` and `Box`:
    /// a handle to a derived struct is a handle to its base, as `&Button` is a `&UiElement`
    /// (spec §14.5, 18.64). The handles keep what they need to drop the value as the type it
    /// was made as, and codegen gives each handle instance a converting constructor.
    fn handle_upcast(&self, expected: &Ty, actual: &Ty) -> bool {
        let (Ty::Named { sym: want, args: wa }, Ty::Named { sym: have, args: ha }) =
            (expected, actual)
        else {
            return false;
        };
        let symbol = self.cx.res.symbol(*want);
        if want != have
            || symbol.module != self.cx.res.prelude
            || !matches!(symbol.name.as_str(), "Rc" | "Weak" | "Box")
        {
            return false;
        }
        match (wa.as_slice(), ha.as_slice()) {
            ([Ty::Named { sym: base, args: ba }], [Ty::Named { sym: derived, args: da }])
                if ba.is_empty() && da.is_empty() =>
            {
                self.cx.table.inherits_from(*derived, *base)
            }
            _ => false,
        }
    }

    /// A trait object needs its implementation's vtable, and only a non-generic
    /// implementation has one yet: `impl<T> Shape for Box<T>` would need one per instance.
    fn refuse_generic_impl_object(&mut self, expected: &Ty, actual: &Ty, span: Span) {
        let (Some(want), Some(have)) = (pointee(expected), pointee(actual)) else { return };
        let Ty::Named { sym, args } = want else { return };
        if self.cx.res.symbol(*sym).kind != SymbolKind::Trait {
            return;
        }
        let r = TraitRef { trait_sym: *sym, args: args.clone(), span: Span::DUMMY };
        let Some((idx, _)) = self.cx.table.impl_of(&r, have) else { return };
        if self.cx.table.impls()[idx].generics.is_empty() {
            return;
        }
        self.cx.error_help(
            codes::CODEGEN_UNSUPPORTED,
            span,
            "a trait object of a type whose implementation is generic is not supported yet",
            "implemented by a generic `impl`",
            "owed by M12 — pass a type with a non-generic `impl`, or use a generic parameter \
             bounded by the trait (spec §7.6)",
        );
    }

    /// Would a by-value `actual` fit `expected` if slicing were allowed? Used only to tell
    /// someone who wrote one what to write instead.
    fn would_slice(&self, expected: &Ty, actual: &Ty) -> bool {
        match (expected, actual) {
            (Ty::Named { sym: base, args: ba }, Ty::Named { sym: derived, args: da })
                if ba.is_empty() && da.is_empty() =>
            {
                self.cx.table.inherits_from(*derived, *base)
            }
            _ => false,
        }
    }

    fn expect(&mut self, expected: &Ty, actual: &Ty, span: Span) {
        let expected = self.subst(expected);
        let actual = self.subst(actual);
        if assignable(&expected, &actual) {
            return;
        }
        if self.base_convertible(&expected, &actual) {
            self.refuse_generic_impl_object(&expected, &actual, span);
            return;
        }
        if self.handle_upcast(&expected, &actual) {
            return;
        }
        // A whole `Derived` where a `Base` is wanted: C++ would silently drop the derived
        // half, which is the one thing 18.32 rules out.
        if self.would_slice(&expected, &actual) {
            let e = self.cx.name_of(&expected);
            let a = self.cx.name_of(&actual);
            self.cx.error_help(
                codes::TYPE_MISMATCH,
                span,
                format!("expected `{e}`, found `{a}`"),
                format!("a whole `{a}`, which is bigger than a `{e}`"),
                format!(
                    "a `{a}` is a `{e}` only behind a reference or a pointer — write \
                     `&…` or `^…`. Copying one into a `{e}` would drop everything `{a}` \
                     adds, which Dazzle does not do silently (spec §7.3)"
                ),
            );
            return;
        }
        let e = self.cx.name_of(&expected);
        let a = self.cx.name_of(&actual);
        // A fallible call that reached a value position was never consumed (spec §9.7).
        if let Ty::Fallible(_) = &actual {
            self.cx.error_help(
                codes::UNUSED_RESULT,
                span,
                "fallible call not consumed",
                "this may fail, and the failure would be discarded",
                "add an `or` clause here: `f() or return`, or `f() or <fallback>` \
                 (spec §9.7)",
            );
            return;
        }
        // `^T` → `&T` is the one conversion that can fail, so it is written out (§5.2).
        if let (
            Ty::Ref { inner: want, .. },
            Ty::Ptr { inner: got, .. } | Ty::OwnPtr { inner: got, .. },
        ) = (&expected, &actual)
        {
            if assignable(want, got) {
                self.cx.error_help(
                    codes::POINTER_NEEDS_CHECK,
                    span,
                    format!("expected `{e}`, found `{a}`"),
                    "a pointer may be null; a reference may not",
                    "check it once where it enters scope: `let x = p or return`, or \
                     `or unreachable` if it provably cannot be null (spec §5.2)",
                );
                return;
            }
        }
        // A numeric mismatch is almost always someone expecting an implicit widening.
        if expected.is_numeric() && actual.is_numeric() {
            self.cx.error_help(
                codes::TYPE_MISMATCH,
                span,
                format!("expected `{e}`, found `{a}`"),
                format!("this is `{a}`"),
                format!(
                    "Dazzle has no implicit numeric conversions, not even widening \
                     (spec §5.1) — write `... as {e}`"
                ),
            );
        } else {
            self.cx.error(
                codes::TYPE_MISMATCH,
                span,
                format!("expected `{e}`, found `{a}`"),
                format!("this is `{a}`"),
            );
        }
    }

    // --- functions --------------------------------------------------------

    fn function(&mut self, f: &FnDecl, sig: &FnSig) -> BlockId {
        self.fallible = sig.fallible;
        if sig.is_unsafe {
            self.unsafe_depth = 1;
        }
        // Signatures are collected before any `Self` is known, so `copy(other: &Self)`
        // arrives here still spelled `Self`. Substituting at the binding is what makes
        // `other.n` resolve to the enclosing struct's field.
        self.ret = self.subst(&self.ret.clone());
        self.push();
        if let Some(is_mut) = sig.takes_self {
            self.self_mutable = is_mut;
            // `self_type_of`, not `Ty::plain`: inside `List<T>`, `self` is a `List<T>`, and a
            // bare `List` would match nothing written with its arguments (`&mut self` where
            // a `^List<T>` is wanted).
            let self_type =
                self.self_full.clone().or_else(|| self.self_ty.map(|s| self.cx.self_type_of(s)));
            if let Some(st) = self_type {
                let id = self.bind("self", st.clone(), is_mut, LocalKind::Param, f.name.span);
                self.self_local = Some(id);
                self.self_local_ty = Some(st.clone());
                // A method body sees its struct's fields unqualified (spec §7.3). They are
                // fields, not locals, so they bind to a slot that remembers the index. An
                // implementation outside the struct's module sees only the `pub` ones.
                if let Ty::Named { sym, args } = &st {
                    let privileged = self.self_ty == Some(*sym);
                    let module = self.cx.module;
                    let fields: Vec<(usize, String, Ty)> = match self.cx.table.struct_sig(*sym) {
                        Some(s) => {
                            let tys = self.cx.table.field_types(*sym, args, &s.fields);
                            s.fields
                                .iter()
                                .zip(tys)
                                .enumerate()
                                .filter(|(_, (x, _))| match x.declared_in {
                                    // The type's own field: its methods see all of them.
                                    None => privileged || x.vis == Vis::Public,
                                    // A base's. Inheriting is not the same as being the
                                    // type, so a base's `private` field stays the base's
                                    // business; `protected` is exactly what opens one up
                                    // to a derived type (spec §4, §7.3, 18.32).
                                    Some(b) => match x.vis {
                                        Vis::Public | Vis::Protected => true,
                                        Vis::Module => self.cx.res.symbol(b).module == module,
                                        Vis::Private => false,
                                    },
                                })
                                .map(|(i, (x, t))| (i, x.name.clone(), t))
                                .collect()
                        }
                        None => Vec::new(),
                    };
                    for (index, name, ty) in fields {
                        self.bind_self_field(&name, ty, is_mut, index);
                    }
                }
            }
        }
        for (p, ps) in f.params.iter().zip(&sig.params) {
            let ty = self.subst(&ps.ty);
            // A default on a variadic parameter was reported where the signature was built;
            // checking it against the `[]T` as well would report the one mistake twice.
            let default = p.default.as_ref().filter(|_| !p.variadic).map(|default| {
                let c = self.expr(default, Some(&ty));
                self.expect(&ty, &c.ty, default.span);
                c.id
            });
            self.hir.param_default(default);
            // Parameters are immutable bindings; mutate through `&mut` or `^`.
            self.bind(&ps.name, ty, false, LocalKind::Param, p.name.span);
        }
        // A constructor's `: Base(…)`, checked here because C++ runs it before the body
        // and its arguments see the constructor's parameters — which are in scope exactly
        // now, and go out of scope at the `pop` below (spec §7.3, 18.32).
        self.base_initialiser(f);
        let root = match &f.body {
            Some(body) => self.block(body, None).id,
            None => self.hir.block(f.name.span, Vec::new()),
        };
        self.pop();
        root
    }

    /// Check a constructor's base initialiser, and require one where the base needs it.
    ///
    /// A base with a constructor of its own has to be built through it: C++ would
    /// otherwise default-initialise the base subobject and skip whatever that constructor
    /// establishes (spec §7.3, 18.32).
    fn base_initialiser(&mut self, f: &FnDecl) {
        let Some(owner) = self.self_ty else { return };
        // Only a constructor has one; `parse_ctor` is the only place that fills it in.
        let base = self.cx.table.struct_sig(owner).and_then(|s| s.base);

        let Some(init) = &f.base_init else {
            // Nothing written. Fine unless the base insists.
            let Some(b) = base else { return };
            let needs = self.cx.table.struct_sig(b).is_some_and(|s| !s.ctors.is_empty());
            // Only a constructor builds the base; a method of the same type must not be
            // asked to.
            if !needs || !self.hir.owner_is_ctor() {
                return;
            }
            let bname = self.cx.res.symbol(b).name.clone();
            let oname = self.cx.res.symbol(owner).name.clone();
            self.cx.error_help(
                codes::MISSING_BASE_INIT,
                f.name.span,
                format!("`{oname}` must construct its base `{bname}`"),
                "no base initialiser",
                format!(
                    "`{bname}` has a constructor, so it cannot be left to default — write \
                     `{oname}(…) : {bname}(…) {{ … }}` (spec §7.3)"
                ),
            );
            return;
        };

        let Some(b) = base else {
            let oname = self.cx.res.symbol(owner).name.clone();
            self.cx.error_help(
                codes::BAD_BASE,
                init.span,
                format!("`{oname}` has no base to initialise"),
                "no base to construct",
                format!(
                    "give `{oname}` a base — `struct {oname} : Base` — or drop this \
                         initialiser (spec §7.3)"
                ),
            );
            for a in &init.args {
                self.expr(a, None);
            }
            return;
        };

        // The path must name the declared base. Anything else would read as constructing
        // something this type does not contain.
        let named = self.path_symbol(&init.path);
        if named != Some(b) {
            let bname = self.cx.res.symbol(b).name.clone();
            let shown = dz_ast::dump::path_str(&init.path);
            self.cx.error_help(
                codes::BAD_BASE,
                init.path.span,
                format!("`{shown}` is not this type's base"),
                format!("its base is `{bname}`"),
                format!(
                    "a base initialiser constructs the one base the struct declares: \
                         write `: {bname}(…)` (spec §7.3)"
                ),
            );
            for a in &init.args {
                self.expr(a, None);
            }
            return;
        }

        let ctors = self.cx.table.struct_sig(b).map(|s| s.ctors.clone()).unwrap_or_default();
        if ctors.is_empty() {
            // `: Base()` on a base with no constructor is default-initialisation, which is
            // what happens anyway — allowed when it is written as such, and nothing is
            // recorded, so codegen leaves C++ to value-initialise the subobject (§6.2).
            if !init.args.is_empty() {
                let bname = self.cx.res.symbol(b).name.clone();
                self.cx.error_help(
                    codes::WRONG_ARG_COUNT,
                    init.span,
                    format!("`{bname}` has no constructor"),
                    "nothing takes these arguments",
                    format!(
                        "`{bname}`'s fields take their defaults; write `: {bname}()`, \
                             or give `{bname}` a constructor (spec §6.2, §7.3)"
                    ),
                );
            }
            for a in &init.args {
                self.expr(a, None);
            }
            return;
        }

        // A constructor call picks the first constructor, as every other one does today
        // — several constructors on one type are not resolved by argument yet.
        let sig = ctors[0].clone();
        let bname = self.cx.res.symbol(b).name.clone();
        self.in_base_init = true;
        let args = self.check_args(&sig, &init.args, init.span, &bname);
        self.in_base_init = false;
        let ty = Ty::Named { sym: b, args: Vec::new() };
        let callee = hir::Callee::Ctor { owner: b, index: 0 };
        let call = self.hir.expr(init.span, ty, hir::ExprKind::Call { callee, args });
        self.hir.set_base_init(call.id);
    }

    // --- statements -------------------------------------------------------

    /// The type a block produces: its final expression statement, or `void`.
    fn block(&mut self, b: &Block, expected: Option<&Ty>) -> CheckedBlock {
        self.block_valued(b, expected, expected.is_some())
    }

    /// A `return` in `: Base(…)`: the base is built before the constructor body starts, so
    /// there is no constructor yet to return from and no object to leave half-built. True
    /// when it was reported (spec §7.3).
    fn refuse_return_in_base_init(&mut self, span: Span) -> bool {
        if !self.in_base_init {
            return false;
        }
        self.cx.error_help(
            codes::RETURN_IN_BASE_INIT,
            span,
            "a base initialiser cannot `return`",
            "the base is built before the constructor begins",
            "give the argument a value to fall back on — `x or 0`, `x or { … }` — or \
             `panic` (spec §7.3)",
        );
        true
    }

    /// Whether the expression starting here has its value used: something expects a type
    /// of it, or it is not a statement of its own — `let x = switch …` has no expected type
    /// and still keeps what its arms produce. Taken on entry, before a sub-expression can
    /// reset `stmt_position`.
    fn value_wanted(&mut self, expected: Option<&Ty>) -> bool {
        let statement = std::mem::replace(&mut self.stmt_position, false);
        expected.is_some() || !statement
    }

    /// A block whose final expression is its value exactly when `wanted` says so.
    fn block_valued(&mut self, b: &Block, expected: Option<&Ty>, wanted: bool) -> CheckedBlock {
        self.push();
        let mut result = Ty::Void;
        let mut stmts = Vec::new();
        let last = b.stmts.len().saturating_sub(1);
        for (i, s) in b.stmts.iter().enumerate() {
            if i == last {
                if let StmtKind::Expr(e) = &s.kind {
                    // The final expression is the block's value only when something wants
                    // one; otherwise it is an ordinary statement, and `or ignore` is legal.
                    self.stmt_position = !wanted;
                    // A bare `none` as an arm's value, with nothing yet to say whose `none`:
                    // the `if` or `switch` around it types it once its arms are joined.
                    let defer_none = wanted
                        && matches!(e.kind, ExprKind::None)
                        && !matches!(expected.map(|t| self.subst(t)), Some(Ty::Optional(_)))
                        && !self.pending_nones.is_empty();
                    let c = if defer_none {
                        let c = self.mk(e, ty::pending_none(), hir::ExprKind::NoneOpt);
                        if let Some(scope) = self.pending_nones.last_mut() {
                            scope.push((c.id, e.span));
                        }
                        c
                    } else {
                        self.expr(e, expected)
                    };
                    self.stmt_position = false;
                    // A `T?` that is the block's value is used, not discarded; a fallible
                    // call is unconsumed either way, having nowhere to be kept (spec §9.7).
                    if !wanted || matches!(c.ty, Ty::Fallible(_)) {
                        self.check_unused(&c.ty, e);
                    }
                    result = c.ty;
                    stmts.push(self.hir.stmt(s.span, hir::StmtKind::Expr(c.id)));
                    continue;
                }
            }
            let id = self.stmt(s);
            stmts.push(id);
        }
        self.pop();
        CheckedBlock { ty: result, id: self.hir.block(b.span, stmts) }
    }

    fn stmt(&mut self, s: &Stmt) -> StmtId {
        let kind = match &s.kind {
            // Removed by the driver before this phase (spec §12.10).
            StmtKind::CfgIf(_) => hir::StmtKind::Error,
            StmtKind::Let(l) => {
                let declared = l.ty.as_ref().map(|t| {
                    let t = self.cx.lower_type(t);
                    self.subst(&t)
                });
                let (ty, init) = match (&declared, &l.init) {
                    (Some(d), Some(init)) => {
                        let c = self.expr(init, Some(d));
                        self.expect(d, &c.ty, init.span);
                        self.reject_temporary_slice(d, init, &c.ty);
                        (d.clone(), Some(c.id))
                    }
                    (Some(d), None) => (d.clone(), None),
                    (None, Some(init)) => {
                        let c = self.expr(init, None);
                        if matches!(c.ty, Ty::Void) {
                            self.cx.error(
                                codes::TYPE_MISMATCH,
                                init.span,
                                "cannot bind a value of type `void`",
                                "this expression produces no value",
                            );
                        }
                        // A binding is not an escape hatch for a fallible call: there is
                        // no `T!` to hold, so it must be consumed here (spec §9.7).
                        if matches!(c.ty, Ty::Fallible(_)) {
                            self.cx.error_help(
                                codes::UNUSED_RESULT,
                                init.span,
                                "fallible call not consumed",
                                "a failure cannot be stored, only handled",
                                "add an `or` clause: `let v = f() or return`, or \
                                 `let v = f() or <fallback>` (spec §9.7)",
                            );
                        }
                        (c.ty.defaulted(), Some(c.id))
                    }
                    // The parser already reported `let` with neither.
                    (None, None) => (Ty::Error, None),
                };
                let locals = self.bind_pattern(&l.pat, &ty, l.mutable, LocalKind::Let);
                match (&l.pat.kind, init) {
                    (PatKind::Tuple(_), Some(init)) => hir::StmtKind::LetTuple { locals, init },
                    _ => hir::StmtKind::Let { local: locals.into_iter().next().flatten(), init },
                }
            }
            StmtKind::Assign { target, op, value } => {
                let mut t = self.expr(target, None);
                // Assigning to a `&mut T` writes the `T` it refers to (spec §5.2). A `&T` is
                // typed the same way, so the one error about it is that it is read-only.
                // `b[i] = v` through `IndexMut` is the same: its `index_mut` returns the `&mut`.
                if let (ExprKind::Path(_) | ExprKind::Index { .. }, Ty::Ref { inner, .. }) =
                    (&target.kind, &t.ty)
                {
                    t.ty = (**inner).clone();
                }
                let v = self.expr(value, Some(&t.ty));
                self.require_mutable(target);
                if *op == AssignOp::Assign {
                    self.expect(&t.ty, &v.ty, value.span);
                    self.reject_temporary_slice(&t.ty, value, &v.ty);
                } else {
                    // Compound assignment desugars to `a = a op b` (spec §11.2).
                    if !t.ty.is_wildcard() && !t.ty.is_numeric() {
                        let name = self.cx.name_of(&t.ty);
                        let o = op.as_str();
                        self.cx.error(
                            codes::INVALID_OPERANDS,
                            target.span,
                            format!("`{o}` cannot be applied to `{name}`"),
                            "not a numeric type",
                        );
                    }
                    self.expect(&t.ty, &v.ty, value.span);
                }
                hir::StmtKind::Assign { target: t.id, op: *op, value: v.id }
            }
            StmtKind::Expr(e) => {
                self.stmt_position = true;
                let c = self.expr(e, None);
                self.stmt_position = false;
                self.check_unused(&c.ty, e);
                hir::StmtKind::Expr(c.id)
            }
            StmtKind::While(w) => {
                let c = self.expr(&w.cond, Some(&Ty::Bool));
                self.require_bool(&c.ty, w.cond.span, "while");
                let (body, label) = self.loop_body(w.label.as_ref(), |b| b.block(&w.body, None).id);
                hir::StmtKind::While { cond: c.id, body, label }
            }
            StmtKind::For(f) => self.for_stmt(f),
            StmtKind::Defer(inner) => hir::StmtKind::Defer(self.stmt(inner)),
            // Nested declarations are checked by the top-level walk.
            StmtKind::Item(_) | StmtKind::Error => hir::StmtKind::Error,
        };
        self.hir.stmt(s.span, kind)
    }

    // --- `for` (spec §10.4) ---------------------------------------------------

    fn for_stmt(&mut self, f: &ForStmt) -> hir::StmtKind {
        // `for i, e in xs.indexed()`: the adapter is the loop's, not a method on `xs`, so
        // `xs` is what is iterated. A type with a real `indexed` method of its own keeps it.
        let mut subject_ast = &f.iter;
        let mut indexed = false;
        if let ExprKind::Call { callee, args } = &f.iter.kind {
            if let ExprKind::Field { base, name, .. } = &callee.kind {
                if name.name == "indexed"
                    && args.is_empty()
                    && !self.has_own_method(base, "indexed")
                {
                    subject_ast = base;
                    indexed = true;
                }
            }
        }
        let consuming = matches!(subject_ast.kind, ExprKind::Move(_));
        if consuming && indexed {
            self.cx.error_help(
                codes::CODEGEN_UNSUPPORTED,
                f.iter.span,
                "`move(…).indexed()` is not supported yet",
                "a consuming loop with a pass count",
                "count the passes yourself, or iterate by reference with `&e` — see the \
                 deferral register in docs/PROGRESS.md",
            );
        }

        let reached_into = std::mem::replace(&mut self.reaching_into, true);
        let mut subject = self.expr(subject_ast, None);
        self.reaching_into = reached_into;
        // A container is looped over through the iterator it hands out (spec §10.4, 18.51).
        if !consuming {
            let writes = f.bindings.iter().any(|b| b.mode == BindMode::RefMut);
            if let Some(iter) = self.container_iterator(subject_ast, &subject, writes) {
                subject = iter;
            }
        }
        let item = self.for_item(subject_ast, &subject.ty, consuming);
        // `for &mut e in TABLE` writes the static's elements (spec §7.7).
        if f.bindings.iter().any(|b| b.mode == BindMode::RefMut) {
            self.require_unsafe_static_write(subject_ast);
        }

        self.push();
        let (shape, bindings) = if indexed {
            (hir::ForShape::Indexed, self.bind_indexed(f, item))
        } else if f.bindings.len() > 1 {
            (hir::ForShape::Tuple, self.bind_tuple(f, item))
        } else {
            let b = &f.bindings[0];
            let shape = if consuming { hir::ForShape::Consuming } else { hir::ForShape::Single };
            (shape, vec![self.bind_for(b, &item)])
        };
        let (body, label) = self.loop_body(f.label.as_ref(), |b| b.block(&f.body, None).id);
        self.pop();
        hir::StmtKind::For { bindings, subject: subject.id, body, shape, label }
    }

    /// `list.iter()`, or `list.iter_mut()` when a binding writes: the iterator a `for` walks
    /// when its subject is a container rather than an iterator (spec §10.4, 18.51). `None`
    /// for what `for` already walks itself — a range, an array or slice, an `Iterate` — and
    /// for what is not a container at all, which `for_item` then reports.
    fn container_iterator(
        &mut self,
        ast: &Expr,
        subject: &Checked,
        writes: bool,
    ) -> Option<Checked> {
        let ty = self.subst(&subject.ty);
        let peeled = ty.peeled().clone();
        if let Ty::Param(p) = &peeled {
            // `C: Iterable` gives the loop the projection `C::Iter`'s `Item`, which
            // monomorphization cannot yet trace back to the implementer — refused, as a
            // generic `T: Iterate` is, rather than emitted as broken C++.
            let bounded = ["Iterable", "IterableMut"].iter().any(|t| {
                self.cx.prelude_trait(t).is_some_and(|sym| {
                    self.satisfies(
                        &peeled,
                        &TraitRef { trait_sym: sym, args: Vec::new(), span: ast.span },
                    )
                })
            });
            if bounded {
                let name = p.name.clone();
                self.cx.error_help(
                    codes::CODEGEN_UNSUPPORTED,
                    ast.span,
                    format!("`for` over a generic `{name}: Iterable` is not supported yet"),
                    "its element type would be the projection of `Iter`'s `Item`",
                    "loop over a concrete container, or pass a slice — see the deferral register \
                     in docs/PROGRESS.md",
                );
                return Some(Checked { ty: Ty::Error, id: subject.id });
            }
            return None;
        }
        if ty.is_wildcard() || matches!(peeled, Ty::Range(_)) || ty.element().is_some() {
            return None;
        }
        let iterate = self.cx.prelude_trait("Iterate")?;
        let as_iterate = TraitRef { trait_sym: iterate, args: Vec::new(), span: ast.span };
        if self.satisfies(&peeled, &as_iterate) {
            return None;
        }
        let (trait_name, method) =
            if writes { ("IterableMut", "iter_mut") } else { ("Iterable", "iter") };
        let trait_sym = self.cx.prelude_trait(trait_name)?;
        let r = TraitRef { trait_sym, args: Vec::new(), span: ast.span };
        if !self.satisfies(&peeled, &r) {
            // Readable but not writable: say that, rather than "cannot be iterated".
            let readable = self.cx.prelude_trait("Iterable").is_some_and(|t| {
                self.satisfies(
                    &peeled,
                    &TraitRef { trait_sym: t, args: Vec::new(), span: ast.span },
                )
            });
            if writes && readable {
                let n = self.cx.name_of(&peeled);
                self.cx.error_help(
                    codes::NOT_MUTABLE,
                    ast.span,
                    format!("`{n}` can be looped over, but not written through"),
                    "implements `Iterable`, not `IterableMut`",
                    "bind each element with `&e` to read it, or implement `IterableMut` \
                     (spec §10.4)",
                );
                return Some(Checked { ty: Ty::Error, id: subject.id });
            }
            return None;
        }
        let sig = self.cx.table.trait_method(&r, &peeled, method)?;
        if writes {
            let writable = match &subject.ty {
                Ty::Ref { mutable, .. } | Ty::Ptr { mutable, .. } | Ty::OwnPtr { mutable, .. } => {
                    *mutable
                }
                _ => self.place_mutability(ast).unwrap_or(true),
            };
            if writable {
                self.require_unsafe_static_write(ast);
            } else {
                // Said as the loop, not as the `iter_mut` call the loop stands for.
                self.cx.error_help(
                    codes::NOT_MUTABLE,
                    ast.span,
                    "cannot refer to these elements with `&mut`",
                    "the container is read-only here",
                    "declare it `let mut`, or pass it as `&mut` (spec §10.4)",
                );
            }
        }
        let ret = self.subst(&sig.ret);
        let callee = hir::Callee::Method {
            recv: subject.id,
            owner: trait_sym,
            name: method.to_owned(),
            method_args: Vec::new(),
        };
        Some(self.hir.expr(ast.span, ret, hir::ExprKind::Call { callee, args: Vec::new() }))
    }

    /// Check a loop's body with the loop on the stack `break` and `continue` resolve
    /// against, and hand back the label C++ needs if a nested loop jumps to this one.
    fn loop_body(
        &mut self,
        label: Option<&Ident>,
        check: impl FnOnce(&mut Self) -> BlockId,
    ) -> (BlockId, Option<hir::LoopLabel>) {
        let id = self.next_loop;
        self.next_loop += 1;
        self.loops.push(LoopFrame {
            id,
            label: label.map(|l| l.name.clone()),
            breaks: false,
            continues: false,
        });
        let body = check(self);
        let frame = self.loops.pop().expect("pushed above");
        let label = (frame.breaks || frame.continues).then(|| hir::LoopLabel {
            id,
            name: frame.label.unwrap_or_default(),
            breaks: frame.breaks,
            continues: frame.continues,
        });
        (body, label)
    }

    /// `break` / `continue`, optionally naming a loop (spec §10.2). `None` targets the
    /// innermost loop; `Some(id)` a loop further out, which C++ reaches with a `goto`.
    fn loop_jump(&mut self, label: Option<&Ident>, span: Span, is_break: bool) -> Option<u32> {
        let word = if is_break { "break" } else { "continue" };
        if self.loops.is_empty() {
            self.cx.error_help(
                codes::JUMP_OUTSIDE_LOOP,
                span,
                format!("`{word}` outside a loop"),
                "not inside a `for` or `while`",
                format!("`{word}` leaves or restarts the loop it is written in (spec §10.2)"),
            );
            return None;
        }
        let label = label?;
        let Some(depth) = self.loops.iter().rposition(|f| f.label.as_deref() == Some(&label.name))
        else {
            let known: Vec<String> = self
                .loops
                .iter()
                .filter_map(|f| f.label.as_ref().map(|l| format!("`{l}`")))
                .collect();
            let help = if known.is_empty() {
                format!("label a loop to name it: `{}: for … {{ … }}` (spec §10.2)", label.name)
            } else {
                format!("the enclosing loops are labelled {} (spec §10.2)", known.join(", "))
            };
            self.cx.error_help(
                codes::JUMP_OUTSIDE_LOOP,
                label.span,
                format!("no enclosing loop is labelled `{}`", label.name),
                "unknown label",
                help,
            );
            return None;
        };
        if depth == self.loops.len() - 1 {
            return None;
        }
        let frame = &mut self.loops[depth];
        if is_break {
            frame.breaks = true;
        } else {
            frame.continues = true;
        }
        Some(frame.id)
    }

    /// Does `base`'s type declare a method called `name` itself?
    fn has_own_method(&mut self, base: &Expr, name: &str) -> bool {
        let ty = self.expr_quiet(base);
        match ty.peeled() {
            Ty::Named { sym, .. } => {
                self.cx.table.struct_sig(*sym).is_some_and(|s| s.methods.contains_key(name))
            }
            _ => false,
        }
    }

    /// What one pass of `for` over `ty` yields.
    fn for_item(&mut self, subject: &Expr, ty: &Ty, consuming: bool) -> ForItem {
        if let Ty::Range(elem) = ty.peeled() {
            return ForItem::Value((**elem).clone());
        }
        if let Some(elem) = ty.element() {
            let elem = elem.clone();
            if consuming {
                return ForItem::Value(elem);
            }
            let writable = self.sequence_writable(subject, ty);
            return ForItem::Place { ty: elem, writable };
        }
        match self.iterate_item(ty, subject.span) {
            // An iterator that hands out references yields places; one that hands out
            // values yields values (spec §8.5).
            Some(Ty::Ref { mutable, inner }) => {
                ForItem::Place { ty: (*inner).clone(), writable: mutable }
            }
            Some(item) => ForItem::Value(item),
            None => {
                if !ty.is_wildcard() {
                    let t = self.cx.name_of(ty);
                    self.cx.error_help(
                        codes::TYPE_MISMATCH,
                        subject.span,
                        format!("`{t}` cannot be iterated"),
                        "not iterable",
                        "`for` works over ranges, arrays, slices, and any type that implements \
                         `Iterate` (spec §8.5, §10.4)",
                    );
                }
                ForItem::Value(Ty::Error)
            }
        }
    }

    /// May the elements of this array or slice be written? A slice says so in its type; an
    /// array is written as its binding allows, or as the reference or pointer it is behind.
    fn sequence_writable(&mut self, subject: &Expr, ty: &Ty) -> bool {
        match ty {
            Ty::Slice { mutable, .. } => *mutable,
            Ty::Ref { mutable, inner }
            | Ty::Ptr { mutable, inner }
            | Ty::OwnPtr { mutable, inner } => {
                *mutable
                    && match &**inner {
                        Ty::Slice { mutable, .. } => *mutable,
                        _ => true,
                    }
            }
            Ty::Array { .. } => self.place_mutability(subject) == Some(true),
            _ => false,
        }
    }

    /// One binding of a `for`: `e` copies what it names, `&e` refers to it, `&mut e` refers
    /// to it and may write (spec §10.4).
    fn bind_for(&mut self, b: &ForBinding, item: &ForItem) -> Option<LocalId> {
        let name = match &b.pat.kind {
            PatKind::Ident(i) => Some(i),
            PatKind::Wild => None,
            PatKind::Tuple(_) => {
                self.cx.error_help(
                    codes::BAD_BINDING,
                    b.pat.span,
                    "a `for` names the parts of a tuple element without parentheses",
                    "parenthesised",
                    "write `for a, b in xs` (spec §10.4)",
                );
                return None;
            }
            PatKind::Error => return None,
        };
        let ty = match (b.mode, item) {
            (BindMode::Value, ForItem::Value(t)) => t.clone(),
            (BindMode::Value, ForItem::Place { ty, .. }) => {
                if !ty.is_wildcard() && !self.cx.table.copyable(ty) {
                    let t = self.cx.name_of(ty);
                    let n = name.map_or("e".to_owned(), |i| i.name.clone());
                    self.cx.error_help(
                        codes::IMPLICIT_COPY,
                        b.span,
                        format!("`{n}` would be a copy of a `{t}`, which cannot be copied"),
                        "a copy of each element",
                        format!(
                            "refer to each element with `&{n}`, or consume the sequence with \
                             `move(…)` (spec §6.3, §10.4)"
                        ),
                    );
                    Ty::Error
                } else {
                    ty.clone()
                }
            }
            (BindMode::Ref | BindMode::RefMut, ForItem::Value(t)) => {
                if !t.is_wildcard() {
                    let shown = self.cx.name_of(t);
                    let n = name.map_or("e".to_owned(), |i| i.name.clone());
                    self.cx.error_help(
                        codes::BAD_BINDING,
                        b.span,
                        format!(
                            "each pass yields a value of type `{shown}`, with nothing to refer to"
                        ),
                        format!("`{}` needs a place", b.mode.prefix().trim_end()),
                        format!(
                            "write `{n}`: a range, a consuming loop and an iterator whose `get` \
                             returns a value all hand out values (spec §10.4)"
                        ),
                    );
                    Ty::Error
                } else {
                    // The element was already reported; bind as asked, so writes in the body
                    // are not reported a second time (invariant 6).
                    Ty::reference(b.mode == BindMode::RefMut, Ty::Error)
                }
            }
            (BindMode::Ref, ForItem::Place { ty, .. }) => Ty::reference(false, ty.clone()),
            (BindMode::RefMut, ForItem::Place { ty, writable: true }) => {
                Ty::reference(true, ty.clone())
            }
            (BindMode::RefMut, ForItem::Place { ty, writable: false }) => {
                if !ty.is_wildcard() {
                    self.cx.error_help(
                        codes::NOT_MUTABLE,
                        b.span,
                        "cannot refer to these elements with `&mut`",
                        "the sequence is read-only here",
                        "declare it `let mut`, pass it as `&mut` or `[]mut T`, or use an \
                         iterator whose `get` returns `&mut` (spec §10.4)",
                    );
                }
                // Bound as asked, so the writes in the body are not each reported again:
                // the mistake is here, once (invariant 6).
                Ty::reference(true, ty.clone())
            }
        };
        name.map(|i| self.bind(&i.name, ty, false, LocalKind::Binding, b.pat.span))
    }

    /// `for a, b, c in xs`: one binding per part of a tuple element (spec §10.4). A part
    /// that is itself a reference — an ECS query's `(Entity, &Button, &mut State)` — is a
    /// place of its own; the rest are places only if the tuple is.
    fn bind_tuple(&mut self, f: &ForStmt, item: ForItem) -> Vec<Option<LocalId>> {
        let (tuple, writable) = match &item {
            ForItem::Value(t) => (t.clone(), None),
            ForItem::Place { ty, writable } => (ty.clone(), Some(*writable)),
        };
        let parts = match &tuple {
            Ty::Tuple(parts) if parts.len() == f.bindings.len() => parts.clone(),
            other => {
                if !other.is_wildcard() {
                    let shown = self.cx.name_of(other);
                    let n = f.bindings.len();
                    let (label, help) = match other {
                        Ty::Tuple(parts) => (
                            format!("{n} names for {} parts", parts.len()),
                            format!("name each part once: the element is `{shown}` (spec §10.4)"),
                        ),
                        _ => (
                            "not a tuple".to_owned(),
                            "several names take apart a tuple element; for a pass count write \
                             `for i, e in xs.indexed()` (spec §10.4)"
                                .to_owned(),
                        ),
                    };
                    let span = f.bindings[0].span.to(f.bindings[n - 1].span);
                    self.cx.error_help(
                        codes::BAD_BINDING,
                        span,
                        format!("cannot take a `{shown}` apart into {n} names"),
                        label,
                        help,
                    );
                }
                return f
                    .bindings
                    .iter()
                    .map(|b| self.bind_for(b, &ForItem::Value(Ty::Error)))
                    .collect();
            }
        };
        f.bindings
            .iter()
            .zip(parts)
            .map(|(b, part)| {
                let part_item = match (part, writable) {
                    (Ty::Ref { mutable, inner }, _) => {
                        ForItem::Place { ty: (*inner).clone(), writable: mutable }
                    }
                    (t, Some(w)) => ForItem::Place { ty: t, writable: w },
                    (t, None) => ForItem::Value(t),
                };
                self.bind_for(b, &part_item)
            })
            .collect()
    }

    /// `for i, e in xs.indexed()`: a pass count, then the element (spec §10.4).
    fn bind_indexed(&mut self, f: &ForStmt, item: ForItem) -> Vec<Option<LocalId>> {
        if f.bindings.len() != 2 {
            let n = f.bindings.len();
            let span = f.bindings[0].span.to(f.bindings[n - 1].span);
            self.cx.error_help(
                codes::BAD_BINDING,
                span,
                format!("`indexed()` yields a pass count and an element, but {n} names are given"),
                "expected two names",
                "write `for i, e in xs.indexed()` (spec §10.4)",
            );
            return f
                .bindings
                .iter()
                .map(|b| self.bind_for(b, &ForItem::Value(Ty::Error)))
                .collect();
        }
        let counter = &f.bindings[0];
        if counter.mode != BindMode::Value {
            self.cx.error_help(
                codes::BAD_BINDING,
                counter.span,
                "the pass count is a value",
                "nothing to refer to",
                "write the counter without `&` (spec §10.4)",
            );
        }
        let count = self.bind_for(
            &ForBinding { mode: BindMode::Value, ..counter.clone() },
            &ForItem::Value(Ty::Int(IntTy::Usize)),
        );
        vec![count, self.bind_for(&f.bindings[1], &item)]
    }

    /// Bind a pattern, returning the locals it introduced in source order. `None` is a
    /// `_`, which binds nothing but still occupies its position.
    fn bind_pattern(
        &mut self,
        p: &Pat,
        ty: &Ty,
        mutable: bool,
        kind: LocalKind,
    ) -> Vec<Option<LocalId>> {
        match &p.kind {
            PatKind::Ident(i) => {
                vec![Some(self.bind(&i.name, ty.clone(), mutable, kind, p.span))]
            }
            PatKind::Tuple(elems) => match ty {
                Ty::Tuple(parts) if parts.len() == elems.len() => {
                    let parts = parts.clone();
                    let mut out = Vec::new();
                    for (e, t) in elems.iter().zip(&parts) {
                        out.extend(self.bind_pattern(e, t, mutable, kind));
                    }
                    out
                }
                _ => {
                    if !ty.is_wildcard() {
                        let t = self.cx.name_of(ty);
                        let n = elems.len();
                        self.cx.error(
                            codes::TYPE_MISMATCH,
                            p.span,
                            format!("cannot destructure `{t}` into {n} bindings"),
                            "not a tuple of the right size",
                        );
                    }
                    let mut out = Vec::new();
                    for e in elems {
                        out.extend(self.bind_pattern(e, &Ty::Error, mutable, kind));
                    }
                    out
                }
            },
            PatKind::Wild | PatKind::Error => vec![None],
        }
    }

    fn require_bool(&mut self, t: &Ty, span: Span, what: &str) {
        if t.is_wildcard() || matches!(t, Ty::Bool) {
            return;
        }
        let name = self.cx.name_of(t);
        self.cx.error_help(
            codes::CONDITION_NOT_BOOL,
            span,
            format!("`{what}` needs a `bool`, found `{name}`"),
            format!("this is `{name}`"),
            "there is no truthiness in Dazzle — compare explicitly, e.g. `x != 0` or \
             `p != null` (spec §10.1)",
        );
    }

    /// A fallible call must be consumed in the statement it appears in; discarding a `T?`
    /// is a warning (spec §9.7).
    fn check_unused(&mut self, t: &Ty, e: &Expr) {
        // An `or` clause has already consumed it. A block, an `if` and a `switch` have
        // already checked their own last statements, where the unconsumed call is; asking
        // again here reported one call once per enclosing block (invariant 6).
        if matches!(
            e.kind,
            ExprKind::Or { .. }
                | ExprKind::Block(_)
                | ExprKind::Unsafe(_)
                | ExprKind::If(_)
                | ExprKind::Switch(_)
        ) {
            return;
        }
        match t {
            Ty::Fallible(_) => {
                self.cx.error_help(
                    codes::UNUSED_RESULT,
                    e.span,
                    "fallible call not consumed",
                    "this may fail, and the failure is discarded",
                    "add an `or` clause: `or return` to propagate, `or <value>` to fall \
                     back, or `or ignore` to drop it deliberately (spec §9.7)",
                );
            }
            Ty::Optional(_) => {
                let name = self.cx.name_of(t);
                self.cx.diags.push(
                    dz_diag::Diagnostic::warning(codes::UNUSED_RESULT, format!("unused `{name}`"))
                        .at(self.cx.file, e.span, "this may be absent, and that is discarded"),
                );
            }
            _ => {}
        }
    }

    // --- places and mutability --------------------------------------------

    /// Report if `e` is not something that can be assigned to.
    fn require_mutable(&mut self, e: &Expr) {
        match self.place_mutability(e) {
            Some(true) => self.require_unsafe_static_write(e),
            Some(false) => {
                let (desc, help) = self.describe_place(e);
                self.cx.error_help(
                    codes::NOT_MUTABLE,
                    e.span,
                    format!("cannot assign to {desc}"),
                    "not mutable",
                    help,
                );
            }
            None => {
                self.cx.error(
                    codes::NOT_MUTABLE,
                    e.span,
                    "cannot assign to this expression",
                    "not a place",
                );
            }
        }
    }

    /// `Some(mutable)` when `e` denotes a place, `None` when it is a value.
    fn place_mutability(&mut self, e: &Expr) -> Option<bool> {
        match &e.kind {
            ExprKind::Path(p) if p.segments.len() == 1 => {
                let name = &p.segments[0].name.name;
                match self.local(name) {
                    // A `&mut T` cannot be pointed elsewhere, so assigning to one writes
                    // through it — `for &mut e in xs { e += 1 }` (spec §5.2, §10.4).
                    Some(Local { ty: Ty::Ref { mutable: true, .. }, .. }) => Some(true),
                    Some(l) => Some(l.mutable),
                    // A `static` is a variable, written inside `unsafe` (§7.7); a
                    // module-level `const` is never assignable.
                    None => Some(self.static_root(e).is_some()),
                }
            }
            ExprKind::Path(_) if self.static_root(e).is_some() => Some(true),
            // Through a pointer, mutability comes from the pointer, not the binding.
            ExprKind::Deref(inner) => match self.expr_quiet(inner) {
                Ty::Ptr { mutable, .. } | Ty::OwnPtr { mutable, .. } => Some(mutable),
                _ => Some(false),
            },
            ExprKind::Field { base, .. } => {
                let base_ty = self.expr_quiet(base);
                match &base_ty {
                    Ty::Ptr { mutable, .. }
                    | Ty::OwnPtr { mutable, .. }
                    | Ty::Ref { mutable, .. } => Some(*mutable),
                    _ => self.place_mutability(base),
                }
            }
            ExprKind::Index { base, .. } => {
                let base_ty = self.expr_quiet(base);
                match &base_ty {
                    Ty::Ptr { mutable, .. }
                    | Ty::OwnPtr { mutable, .. }
                    | Ty::Ref { mutable, .. } => Some(*mutable),
                    // A slice is a view, so its own binding says nothing about its
                    // elements: writability is in the type (spec §5.3, §18.22).
                    Ty::Slice { mutable, .. } => Some(*mutable),
                    // A fixed array *is* the value, so it follows the binding.
                    _ => self.place_mutability(base),
                }
            }
            _ => None,
        }
    }

    /// The help for writing through a `for` or `case` binding that was not bound to write:
    /// the fix is in the binding, not in a `let mut` (spec §10.3, §10.4).
    fn binding_write_help(&self, name: &str) -> Option<String> {
        let local = self.local(name)?;
        let Slot::Local(id) = local.slot else { return None };
        if self.hir.local_kind(id) != LocalKind::Binding {
            return None;
        }
        if self.if_let_bindings.contains(&id) {
            return Some(format!(
                "`{name}` is a copy of the value, or a read-only reference to it; bind it as \
                 `if let &mut {name}` to change the value where it lives (spec §9.4)"
            ));
        }
        Some(match local.ty.peeled() {
            Ty::Variant(..) => format!(
                "`{name}` is a copy of the payload, or a read-only reference to it; bind it as \
                 `&mut {name}` to change the payload itself (spec §10.3)"
            ),
            _ => format!(
                "`{name}` is a copy of the element, or a read-only reference to it; bind it as \
                 `&mut {name}` to change the element itself (spec §10.4)"
            ),
        })
    }

    fn describe_place(&mut self, e: &Expr) -> (String, String) {
        match &e.kind {
            ExprKind::Path(p) if p.segments.len() == 1 => {
                let name = p.segments[0].name.name.clone();
                if let Some(help) = self.binding_write_help(&name) {
                    (format!("`{name}`"), help)
                } else if self.local(&name).is_some() {
                    (
                        format!("`{name}`"),
                        format!("declare it as `let mut {name}` to allow assignment"),
                    )
                } else {
                    (
                        format!("`{name}`"),
                        "constants and functions cannot be assigned to".to_owned(),
                    )
                }
            }
            ExprKind::Deref(ptr) => self.through_read_only_pointer(ptr),
            ExprKind::Field { base, name, .. } => match self.expr_quiet(base).peeled_refs() {
                Ty::Ptr { mutable: false, .. } | Ty::OwnPtr { mutable: false, .. } => {
                    self.through_read_only_pointer(base)
                }
                _ if matches!(&base.kind, ExprKind::Path(p) if p.segments.len() == 1
                    && self.binding_write_help(&p.segments[0].name.name).is_some()) =>
                {
                    let ExprKind::Path(p) = &base.kind else { unreachable!() };
                    let help =
                        self.binding_write_help(&p.segments[0].name.name).unwrap_or_default();
                    (format!("field `{}`", name.name), help)
                }
                _ => (
                    format!("field `{}`", name.name),
                    "a field is mutable through `&mut self` or a `^mut` pointer; this method \
                     takes `&self`, so make it take `&mut self` (spec §7.3)"
                        .to_owned(),
                ),
            },
            ExprKind::Index { base, .. } => match self.expr_quiet(base) {
                Ty::Ptr { mutable: false, .. } | Ty::OwnPtr { mutable: false, .. } => {
                    self.through_read_only_pointer(base)
                }
                Ty::Slice { mutable: false, elem } => {
                    let elem = self.cx.name_of(&elem);
                    (
                        "an element of a read-only slice".to_owned(),
                        format!(
                            "`[]{elem}` is a read-only view; write `[]mut {elem}` to allow \
                             assignment through it (spec §5.3)"
                        ),
                    )
                }
                // A fixed array is a value, so the fault is with whatever holds it.
                _ => self.describe_place(base),
            },
            _ => ("this".to_owned(), "not a mutable place".to_owned()),
        }
    }

    /// Is this member access a C array member of an imported struct (spec §15.2)?
    fn is_c_array_member(&self, base_ty: &Ty, field_ty: &Ty) -> bool {
        matches!(field_ty, Ty::Array { .. })
            && base_ty.peeled().named().is_some_and(|sym| {
                self.cx.ws.module(self.cx.res.symbol(sym).module).foreign.is_some()
            })
    }

    /// The name of the imported C bit-field `e` reads, when it reads one.
    fn bitfield_name(&mut self, e: &Expr) -> Option<String> {
        let ExprKind::Field { base, name, .. } = &e.kind else { return None };
        let base_ty = self.expr_quiet(base);
        let sym = base_ty.peeled().named()?;
        let sig = self.cx.table.struct_sig(sym)?;
        sig.fields.iter().find(|f| f.name == name.name && f.bitfield).map(|f| f.name.clone())
    }

    /// A write through a `^T`, which promises its pointee is only read (spec §5.2, §18.37).
    fn through_read_only_pointer(&mut self, ptr: &Expr) -> (String, String) {
        let t = self.expr_quiet(ptr);
        let n = self.cx.name_of(&t);
        (
            "memory behind a read-only pointer".to_owned(),
            format!("`{n}` is only read through; a `^mut` pointer allows writing (spec §5.2)"),
        )
    }

    /// `construct` and `destroy` write the place they are given (spec §14.3), so it has to
    /// be one that may be written — `m_data[i]` through an `own ^mut T`, not through a `^T`.
    fn require_writable(&mut self, e: &Expr, what: &str) {
        if self.place_mutability(e) == Some(false) {
            let (desc, help) = self.describe_place(e);
            self.cx.error_help(
                codes::NOT_MUTABLE,
                e.span,
                format!("`{what}` cannot write to {desc}"),
                "not writable",
                help,
            );
        }
    }

    /// Type of an expression without emitting diagnostics or keeping nodes, for place
    /// analysis. The expression has already been checked once; this is a second look.
    fn expr_quiet(&mut self, e: &Expr) -> Ty {
        let before = self.cx.diags.len();
        let reported = self.unsafe_reported.len();
        let mark = self.hir.mark();
        let t = self.expr(e, None).ty;
        self.hir.rollback(mark);
        self.cx.diags.truncate(before);
        // A probe that ran before the real check must not use up its diagnostic.
        self.unsafe_reported.truncate(reported);
        t
    }

    // --- expressions ------------------------------------------------------

    /// Check `e`, then apply the one rule that needs both a type and a place.
    ///
    /// Coercing a fixed array to a `[]mut T` hands out write access to the array's
    /// elements, so the array itself has to be writable — a question `assignable` cannot
    /// answer, because it sees types and not places (spec §5.3, §18.22). Every context
    /// that supplies an expected type goes through here, so the rule is stated once.
    ///
    /// The other coercion that needs a node is reading a `&T` where a `T` is wanted: it
    /// copies the referent (spec §5.2, §18.35). Made explicit as a `Deref` so flow sees the
    /// read — and reports `DZ0402` when `T` is move-only — and codegen sees the copy.
    fn expr(&mut self, e: &Expr, expected: Option<&Ty>) -> Checked {
        let checked = self.expr_inner(e, expected);
        if let Some(exp) = expected {
            let (want, got) = (self.subst(exp), self.subst(&checked.ty));
            // A `T?` is filled from a `T` later, where the value is wrapped, so the read has
            // to look inside it: `return m` into a `Movie?` reads a `Movie` (spec §9.1).
            let value = match &want {
                Ty::Optional(v) => (**v).clone(),
                other => other.clone(),
            };
            if let Ty::Ref { inner, .. } = &got {
                if !want.is_wildcard() && !assignable(&want, &got) && assignable(&value, inner) {
                    return self.hir.expr(
                        e.span,
                        (**inner).clone(),
                        hir::ExprKind::Deref(checked.id),
                    );
                }
            }
            if let (Ty::Slice { mutable: true, .. }, Ty::Array { .. }) =
                (&self.subst(exp), &self.subst(&checked.ty))
            {
                self.require_unsafe_static_write(e);
                if self.place_mutability(e) == Some(false) {
                    let (desc, _) = self.describe_place(e);
                    let want = self.cx.name_of(exp);
                    self.cx.error_help(
                        codes::NOT_MUTABLE,
                        e.span,
                        format!("cannot view {desc} as `{want}`"),
                        "this array is not writable",
                        "a `[]mut T` may be written through, so the array behind it must be \
                         mutable — declare it with `let mut` (spec §5.3)",
                    );
                }
            }
        }
        checked
    }

    fn expr_inner(&mut self, e: &Expr, expected: Option<&Ty>) -> Checked {
        match &e.kind {
            ExprKind::Int(text) => {
                let value = parse_int_literal(text).unwrap_or(0);
                let mut ty = Ty::IntLit;
                // A suffix types the literal whatever the context wants; a context that wants
                // something else reports a mismatch where it is checked (spec §2.5).
                let suffix = crate::literal_suffix_type(text);
                if let Some(Ty::Float(f)) = suffix {
                    return self.mk(e, Ty::Float(f), hir::ExprKind::Float(value as f64));
                }
                let context = match suffix {
                    Some(t) => Some(t),
                    None => expected.map(|t| self.subst(t)),
                };
                if let (Some(exp), Some(v)) = (context, parse_int_literal(text)) {
                    if let Ty::Int(i) = exp {
                        let (lo, hi) = i.range();
                        if v < lo || v > hi {
                            let n = i.name();
                            self.cx.error_help(
                                codes::LITERAL_OUT_OF_RANGE,
                                e.span,
                                format!("`{v}` does not fit in `{n}`"),
                                format!("outside the range of `{n}`"),
                                format!("valid values are {lo} to {hi}"),
                            );
                            ty = Ty::Error;
                        } else {
                            ty = Ty::Int(i);
                        }
                    } else if let Ty::Float(f) = exp {
                        // An integer literal in a float context is a float (spec §2.5).
                        return self.mk(e, Ty::Float(f), hir::ExprKind::Float(value as f64));
                    }
                }
                self.mk(e, ty, hir::ExprKind::Int(value))
            }
            ExprKind::Float(text) => {
                let ty = match (crate::literal_suffix_type(text), expected.map(|t| self.subst(t))) {
                    (Some(t), _) => t,
                    (None, Some(Ty::Float(f))) => Ty::Float(f),
                    _ => Ty::FloatLit,
                };
                self.mk(e, ty, hir::ExprKind::Float(parse_float_literal(text)))
            }
            // A string literal is a `cstr` wherever a `cstr` is wanted: it already sits in
            // static storage with a zero after its last byte, so nothing is copied and C++
            // sees the literal itself (spec §5.3, §18.39).
            ExprKind::Str(text) => {
                let ty = match expected.map(|t| self.subst(t)) {
                    Some(Ty::CStr) => Ty::CStr,
                    _ => Ty::Str,
                };
                let kind = hir::ExprKind::Str(decode_string_literal(text));
                self.mk(e, ty, kind)
            }
            ExprKind::RawStr(text) => {
                let ty = match expected.map(|t| self.subst(t)) {
                    Some(Ty::CStr) => Ty::CStr,
                    _ => Ty::Str,
                };
                let kind = hir::ExprKind::Str(decode_raw_string_literal(text));
                self.mk(e, ty, kind)
            }
            ExprKind::Char(text) => {
                let c = decode_string_literal(text).chars().next().unwrap_or('\0');
                self.mk(e, Ty::Char, hir::ExprKind::Char(c))
            }
            ExprKind::Bool(b) => self.mk(e, Ty::Bool, hir::ExprKind::Bool(*b)),
            ExprKind::Null => {
                let ty = match expected.map(|t| self.subst(t)) {
                    Some(t @ (Ty::Ptr { .. } | Ty::OwnPtr { .. })) => t,
                    // An empty `Rc`, `Weak` or `Box` — the handle's default (spec §14.5).
                    Some(t) if self.nullable(&t, e.span).is_some() => t,
                    _ => Ty::ptr(true, Ty::Error),
                };
                self.mk(e, ty, hir::ExprKind::Null)
            }
            ExprKind::None => {
                let ty = match expected.map(|t| self.subst(t)) {
                    Some(t @ Ty::Optional(_)) => t,
                    // `none` only means something where a `T?` is wanted (spec §9.1).
                    _ => match &self.ret {
                        Ty::Optional(inner) => Ty::Optional(inner.clone()),
                        _ => {
                            self.cx.error_help(
                                codes::NO_CONTEXT_FOR_LITERAL,
                                e.span,
                                "`none` needs an optional type here",
                                "nothing here expects a `T?`",
                                "`none` is the empty case of `T?` (spec §9.1)",
                            );
                            Ty::Error
                        }
                    },
                };
                self.mk(e, ty, hir::ExprKind::NoneOpt)
            }
            ExprKind::Path(p) => self.path_expr(p, e, expected),
            ExprKind::Tuple(elems) => {
                let checked: Vec<Checked> = match expected.map(|t| self.subst(t)) {
                    Some(Ty::Tuple(want)) if want.len() == elems.len() => {
                        elems.iter().zip(&want).map(|(x, w)| self.expr(x, Some(w))).collect()
                    }
                    _ => elems.iter().map(|x| self.expr(x, None)).collect(),
                };
                let ty = Ty::Tuple(checked.iter().map(|c| c.ty.defaulted()).collect());
                let parts = checked.iter().map(|c| c.id).collect();
                self.mk(e, ty, hir::ExprKind::Tuple(parts))
            }
            ExprKind::Ref { mutable, expr } => {
                // What the reference is wanted to point at is the operand's context, so
                // `map.get(&7)` on a `HashMap<u8, …>` settles the literal as a `u8` (§2.5).
                let wanted = match expected {
                    Some(Ty::Ref { inner, .. }) | Some(Ty::Ptr { inner, .. }) => {
                        Some((**inner).clone())
                    }
                    _ => None,
                };
                let inner = self.expr(expr, wanted.as_ref());
                if *mutable {
                    self.require_mutable(expr);
                }
                // A bit-field is a few bits inside a larger member: C++ has no pointer or
                // reference to one, so neither has Dazzle (spec §15.2).
                if let Some(name) = self.bitfield_name(expr) {
                    self.cx.error_help(
                        codes::BITFIELD_ADDRESS,
                        e.span,
                        format!("`{name}` is a C bit-field, so it has no address"),
                        "cannot be referenced",
                        "read it or assign it directly; what it names is a few bits inside \
                         another member (spec §15.2)",
                    );
                }
                let ty = Ty::Ref { mutable: *mutable, inner: Rc::new(inner.ty.defaulted()) };
                self.mk(e, ty, hir::ExprKind::Ref { mutable: *mutable, place: inner.id })
            }
            ExprKind::Deref(inner) => {
                let c = self.expr(inner, None);
                let ty = match &c.ty {
                    Ty::Ptr { inner: i, .. } | Ty::OwnPtr { inner: i, .. } => {
                        self.require_unsafe(e.span, "dereferencing a `^T`");
                        // A handle has no size here, so there is no value to read out of
                        // the pointer (spec §15.2, §18.40).
                        if self.cx.table.is_opaque(i) {
                            let n = self.cx.name_of(i);
                            self.cx.error_help(
                                codes::OPAQUE_BY_VALUE,
                                e.span,
                                format!("`{n}` cannot be read out of a pointer"),
                                "its size is unknown here",
                                format!(
                                    "its header only declares `{n}`, so it is a handle: pass \
                                     the `^{n}` on, and let the library's own functions work \
                                     on what it points at (spec §15.2)"
                                ),
                            );
                            Ty::Error
                        } else {
                            (**i).clone()
                        }
                    }
                    t if t.is_wildcard() => Ty::Error,
                    t => {
                        let n = self.cx.name_of(t);
                        self.cx.error_help(
                            codes::NOT_DEREFERENCEABLE,
                            e.span,
                            format!("`{n}` cannot be dereferenced"),
                            "not a pointer",
                            "postfix `^` dereferences a `^T`; `.` already reaches through \
                             pointers on its own (spec §5.2)",
                        );
                        Ty::Error
                    }
                };
                self.mk(e, ty, hir::ExprKind::Deref(c.id))
            }
            ExprKind::Move(inner) => {
                let c = self.expr(inner, expected);
                self.mk(e, c.ty.clone(), hir::ExprKind::Move(c.id))
            }
            ExprKind::Unary { op, operand } => {
                // `-128` for an `i8` is one literal, whose range is checked as a whole: the
                // `128` alone does not fit, and checking it first rejected the most negative
                // value of every signed type (spec §2.5).
                if *op == UnaryOp::Neg {
                    if let ExprKind::Int(text) = &operand.kind {
                        let want = match crate::literal_suffix_type(text) {
                            Some(t) => Some(t),
                            None => expected.map(|t| self.subst(t)),
                        };
                        if let (Some(Ty::Int(i)), Some(v)) = (want, parse_int_literal(text)) {
                            let (lo, hi) = i.range();
                            if -v < lo || -v > hi {
                                let n = i.name();
                                self.cx.error_help(
                                    codes::LITERAL_OUT_OF_RANGE,
                                    e.span,
                                    format!("`-{v}` does not fit in `{n}`"),
                                    format!("outside the range of `{n}`"),
                                    format!("valid values are {lo} to {hi}"),
                                );
                                return self.mk(e, Ty::Error, hir::ExprKind::Int(-v));
                            }
                            return self.mk(e, Ty::Int(i), hir::ExprKind::Int(-v));
                        }
                    }
                }
                let c = self.expr(operand, expected);
                let t = c.ty.clone();
                // `-v` on a struct is its `Neg` implementation (spec §11.2).
                if *op == UnaryOp::Neg && self.needs_operator_trait(t.peeled()) {
                    if let Some(call) = self.operator_call("Neg", "neg", &c, None, None, e.span) {
                        return call;
                    }
                    self.operator_needs_trait("-", t.peeled(), "Neg", e.span);
                    return self.mk(e, Ty::Error, hir::ExprKind::Error);
                }
                let ty = match op {
                    UnaryOp::Not => {
                        self.require_bool(&t, operand.span, "`!`");
                        Ty::Bool
                    }
                    UnaryOp::Neg => {
                        if !t.is_wildcard() && !t.is_numeric() {
                            let n = self.cx.name_of(&t);
                            self.cx.error(
                                codes::INVALID_OPERANDS,
                                e.span,
                                format!("`-` cannot be applied to `{n}`"),
                                "not a numeric type",
                            );
                            Ty::Error
                        } else {
                            t
                        }
                    }
                    UnaryOp::BitNot => {
                        if !t.is_wildcard() && !t.is_integral() {
                            let n = self.cx.name_of(&t);
                            self.cx.error(
                                codes::INVALID_OPERANDS,
                                e.span,
                                format!("`~` cannot be applied to `{n}`"),
                                "not an integer type",
                            );
                            Ty::Error
                        } else {
                            t
                        }
                    }
                };
                self.mk(e, ty, hir::ExprKind::Unary { op: *op, operand: c.id })
            }
            ExprKind::Binary { op, lhs, rhs } => self.binary(*op, lhs, rhs, e, expected),
            ExprKind::Cast { expr, ty } => {
                let target = self.cx.lower_type(ty);
                let target = self.subst(&target);
                // `"x" as cstr`: a literal is typed by what it is cast to (spec §18.39). Only
                // for `cstr`, so a numeric literal still meets its cast as `{integer}`.
                let hint = matches!(target, Ty::CStr).then_some(&target);
                let from = self.expr(expr, hint);
                self.check_cast(&from.ty, &target, e.span);
                // Reinterpreting what a pointer points at is the whole of what makes a
                // pointer cast dangerous (spec §5.2, §10.6).
                if matches!(from.ty, Ty::Ptr { .. } | Ty::OwnPtr { .. })
                    && matches!(target, Ty::Ptr { .. })
                {
                    self.require_unsafe(e.span, "a pointer cast");
                }
                self.mk(e, target, hir::ExprKind::Cast { operand: from.id })
            }
            ExprKind::Range { start, end, inclusive } => {
                let start = start.as_ref().map(|x| self.expr(x, None));
                let end = end.as_ref().map(|x| self.expr(x, None));
                let (s, en) =
                    (start.as_ref().map(|c| c.ty.clone()), end.as_ref().map(|c| c.ty.clone()));
                let elem = match (s, en) {
                    (Some(a), Some(b)) => match join(&a, &b) {
                        Some(t) => t.defaulted(),
                        None => {
                            let (x, y) = (self.cx.name_of(&a), self.cx.name_of(&b));
                            self.cx.error(
                                codes::TYPE_MISMATCH,
                                e.span,
                                format!("range bounds differ: `{x}` and `{y}`"),
                                "both ends must have the same type",
                            );
                            Ty::Error
                        }
                    },
                    (Some(a), None) | (None, Some(a)) => a.defaulted(),
                    (None, None) => Ty::Int(IntTy::I32),
                };
                if !elem.is_wildcard() && !elem.is_integral() {
                    let n = self.cx.name_of(&elem);
                    self.cx.error(
                        codes::TYPE_MISMATCH,
                        e.span,
                        format!("a range needs integer bounds, found `{n}`"),
                        "not an integer",
                    );
                }
                let kind = hir::ExprKind::Range {
                    start: start.map(|c| c.id),
                    end: end.map(|c| c.id),
                    inclusive: *inclusive,
                };
                self.mk(e, Ty::Range(Rc::new(elem)), kind)
            }
            ExprKind::Index { base, index }
                if matches!(index.kind, ExprKind::Range { inclusive: false, .. }) =>
            {
                let ExprKind::Range { start, end, .. } = &index.kind else { unreachable!() };
                self.slice_range(e, base, start.as_deref(), end.as_deref())
            }
            ExprKind::Index { base, index } => {
                let reached_into = std::mem::replace(&mut self.reaching_into, true);
                let b = self.expr(base, None);
                self.reaching_into = reached_into;
                let i = self.expr(index, Some(&Ty::Int(IntTy::Usize)));
                if !i.ty.is_wildcard() && !i.ty.is_integral() {
                    let n = self.cx.name_of(&i.ty);
                    self.cx.error(
                        codes::TYPE_MISMATCH,
                        index.span,
                        format!("an index must be an integer, found `{n}`"),
                        "not an integer",
                    );
                }
                // `b[i]` on a user type is its `Index` — or, where the value may be written,
                // its `IndexMut` — which hands back a reference to the element (spec §11.2).
                if let Ty::Named { sym, .. } = b.ty.peeled_refs() {
                    if !matches!(self.cx.table.get(*sym), Some(ItemSig::Enum { .. })) {
                        let writable = match &b.ty {
                            Ty::Ref { mutable, .. } => *mutable,
                            _ => self.place_mutability(base) == Some(true),
                        };
                        if writable {
                            if let Some(c) = self.operator_call(
                                "IndexMut",
                                "index_mut",
                                &b,
                                Some(&i),
                                Some(index),
                                e.span,
                            ) {
                                return c;
                            }
                        }
                        if let Some(c) =
                            self.operator_call("Index", "index", &b, Some(&i), Some(index), e.span)
                        {
                            return c;
                        }
                        let n = self.cx.name_of(&b.ty);
                        self.cx.error_help(
                            codes::NOT_INDEXABLE,
                            base.span,
                            format!("`{n}` cannot be indexed"),
                            "no `Index` implementation",
                            format!("implement `Index` for `{n}`, and `IndexMut` to write through it (spec §11.2)"),
                        );
                        return self.mk(e, Ty::Error, hir::ExprKind::Error);
                    }
                }
                // Indexing a `^T` is pointer arithmetic, as in C: unchecked, since a pointer
                // carries no length. §8.3's own `m_data[i]` on an `own ^T` depends on it.
                // Kept out of `Ty::element` so that `for x in ptr` stays an error — a
                // pointer is indexable but it is not a sequence.
                let pointee = match b.ty.peeled_refs() {
                    Ty::Ptr { inner, .. } | Ty::OwnPtr { inner, .. }
                        if !matches!(**inner, Ty::Array { .. } | Ty::Slice { .. }) =>
                    {
                        Some((**inner).clone())
                    }
                    _ => None,
                };
                if pointee.is_some() {
                    self.require_unsafe(e.span, "indexing a `^T`");
                }
                let ty = match pointee.as_ref().or(b.ty.element()) {
                    Some(t) => t.clone(),
                    None => {
                        if !b.ty.is_wildcard() {
                            let n = self.cx.name_of(&b.ty);
                            self.cx.error(
                                codes::NOT_INDEXABLE,
                                base.span,
                                format!("`{n}` cannot be indexed"),
                                "not an array or slice",
                            );
                        }
                        Ty::Error
                    }
                };
                self.mk(e, ty, hir::ExprKind::Index { base: b.id, index: i.id })
            }
            ExprKind::Field { base, name, .. } => {
                // Whether *this* access is reached into — `s.samples.len` — or taken as a
                // value. The base below is reached into either way (spec §15.2).
                let reached_into = std::mem::replace(&mut self.reaching_into, true);
                let b = self.expr(base, None);
                self.reaching_into = reached_into;
                if matches!(b.ty.peeled_refs(), Ty::Ptr { .. } | Ty::OwnPtr { .. }) {
                    self.require_unsafe(e.span, "reaching through a `^T` with `.`");
                }
                let (ty, index) = self.field_type(&b.ty, name, base.span);
                // An imported C array member is a raw array in C++, which cannot be copied
                // or assigned as a whole — only indexed, iterated or pointed at (§15.2).
                if !reached_into && self.is_c_array_member(&b.ty, &ty) {
                    let n = self.cx.name_of(&ty);
                    self.cx.error_help(
                        codes::C_ARRAY_BY_VALUE,
                        e.span,
                        format!("`{}` is a C array member, so it is not a value here", name.name),
                        format!("a `{n}` in C cannot be copied"),
                        "index it, iterate it with `for`, or take `.ptr`; C has no way to \
                         copy or assign a whole array (spec §15.2)",
                    );
                }
                match index {
                    // Tuples are indexed by position, structs by field (spec §5.4).
                    Some(index) if matches!(b.ty.peeled(), Ty::Tuple(_)) => {
                        self.mk(e, ty, hir::ExprKind::TupleIndex { base: b.id, index })
                    }
                    Some(index) => self.mk(e, ty, hir::ExprKind::Field { base: b.id, index }),
                    // `.len` is the one successful member with no field index: the
                    // container knows its own length (spec §5.3). Every other `None` is a
                    // mistake `field_type` has already reported.
                    None if name.name == "len" && !ty.is_error() => {
                        self.mk(e, ty, hir::ExprKind::Len(b.id))
                    }
                    // `.ptr` is as writable as the view it comes from: a `[]mut T` or a
                    // writable array gives a `^mut T`, anything else a `^T` (spec §5.3).
                    None if name.name == "ptr" && !ty.is_error() => {
                        let writable = match (b.ty.peeled(), &b.ty) {
                            (Ty::Slice { mutable, .. }, _) | (_, Ty::Ref { mutable, .. }) => {
                                *mutable
                            }
                            _ => self.place_mutability(base) != Some(false),
                        };
                        let ty = match ty {
                            Ty::Ptr { inner, .. } => Ty::Ptr { mutable: writable, inner },
                            other => other,
                        };
                        self.mk(e, ty, hir::ExprKind::DataPtr(b.id))
                    }
                    None => self.mk(e, ty, hir::ExprKind::Error),
                }
            }
            ExprKind::Call { callee, args } => self.call(callee, args, e),
            ExprKind::StructLit { path, fields } => {
                // `Shape::Circle{ … }` is a union value, not a struct: the path names a
                // variant, and `Shape::Circle` is not a type of its own (spec §7.5).
                if let Some((owner, index)) = self.union_variant_path(path) {
                    let args = self.variant_args(path, owner, expected, Some((index, fields)));
                    let kind = self.union_variant_literal(owner, &args, index, fields, e.span);
                    return self.mk(e, Ty::Named { sym: owner, args }, kind);
                }
                // `lower_path_type`, not `lower_type`: this is the type a path *names*,
                // not a type written in a declaration, and the by-value checks that
                // `lower_type` adds would report a literal of an abstract type twice —
                // once as "by value" and once as `struct_literal`'s better "cannot be
                // built" (spec §7.3, 18.32).
                let ty = self.cx.lower_path_type(path, path.span);
                let ty = self.subst(&ty);
                let kind = self.struct_literal(&ty, fields, e.span);
                self.mk(e, ty, kind)
            }
            // `[1, 2, 3]` — an array of exactly this many elements (spec §5.3).
            ExprKind::ArrayLit(elems) => self.array_literal(elems, e, expected),
            // `.Variant{}` needs the expected type to know which type it belongs to.
            ExprKind::DotInfer { name, init } => {
                let Some(want) = expected.map(|t| self.subst(t)) else {
                    self.cx.error_help(
                        codes::NO_CONTEXT_FOR_LITERAL,
                        e.span,
                        format!("cannot infer the type of `.{}`", name.name),
                        "no expected type here",
                        "annotate the binding, e.g. `let s: ButtonState = .Idle` (spec §7.3)",
                    );
                    if let Some(fields) = init {
                        for f in fields {
                            self.expr(&f.value, None);
                        }
                    }
                    return self.mk(e, Ty::Error, hir::ExprKind::Error);
                };
                // Where a `T?` is wanted, `.Variant` is a `T`, which becomes the `T?` as any
                // value does (spec §7.4, §9.1, 18.53).
                let want = match want {
                    Ty::Optional(inner) => (*inner).clone(),
                    other => other,
                };
                let kind = self.dot_infer(&want, name, init.as_deref(), e.span);
                self.mk(e, want, kind)
            }
            ExprKind::Block(b) => {
                let wanted = self.value_wanted(expected);
                let c = self.block_valued(b, expected, wanted);
                let block = self.mk(e, c.ty, hir::ExprKind::Block(c.id));
                // `else { none }`: the block's value waits for its type with the `none`.
                if ty::is_pending_none(&block.ty) {
                    if let Some(scope) = self.pending_nones.last_mut() {
                        scope.push((block.id, e.span));
                    }
                }
                block
            }
            // Past checking, an `unsafe` block is an ordinary block: the permission it
            // grants is a compile-time fact, and nothing about it reaches C++ (spec §5.2).
            ExprKind::Unsafe(b) => {
                self.unsafe_depth += 1;
                let c = self.block(b, expected);
                self.unsafe_depth -= 1;
                self.mk(e, c.ty, hir::ExprKind::Block(c.id))
            }
            ExprKind::If(i) => {
                // `if let name = value` (spec §9.4): `name` holds what `or` would produce
                // from the value — the `T` of a `T?`, or the `&mut T` of a `^T` — and exists
                // only in the `then` block.
                // Without an `else` there is no value, however the `if` is used.
                let wanted = self.value_wanted(expected) && i.else_branch.is_some();
                if wanted {
                    self.pending_nones.push(Vec::new());
                }
                let (head, then) = match &i.binding {
                    None => {
                        let cond = self.expr(&i.cond, Some(&Ty::Bool));
                        self.require_bool(&cond.ty, i.cond.span, "if");
                        (Err(cond.id), self.block_valued(&i.then, expected, wanted))
                    }
                    Some(b) => {
                        let subject = self.expr(&i.cond, None);
                        let subject = match self.guarded_handle(&subject, &i.cond) {
                            Some((guarded, _, _)) => guarded,
                            None => subject,
                        };
                        let bound = match &subject.ty {
                            Ty::Optional(inner) => {
                                self.if_let_binding(b, &i.cond, inner, None, false)
                            }
                            // A reference to an optional — what `^T? or return` gives —
                            // reaches the optional it refers to (18.35).
                            Ty::Ref { mutable, inner } if matches!(**inner, Ty::Optional(_)) => {
                                let Ty::Optional(payload) = &**inner else { unreachable!() };
                                self.if_let_binding(b, &i.cond, payload, Some(*mutable), true)
                            }
                            Ty::Ptr { mutable, inner } | Ty::OwnPtr { mutable, inner } => {
                                self.if_let_binding(b, &i.cond, inner, Some(*mutable), true)
                            }
                            t if t.is_wildcard() => Ty::Error,
                            Ty::Fallible(_) => {
                                self.cx.error_help(
                                    codes::IF_LET_FALLIBLE,
                                    i.cond.span,
                                    "`if let` cannot take a fallible call",
                                    "this may fail with an `Error`",
                                    "`else` has no `Error` to hand over, so the failure would be \
                                     dropped unseen — handle it with `or` (spec §9.4)",
                                );
                                Ty::Error
                            }
                            t => {
                                let n = self.cx.name_of(t);
                                self.cx.error_help(
                                    codes::NOT_FALLIBLE,
                                    i.cond.span,
                                    format!("`if let` needs a `T?` or a `^T`, found `{n}`"),
                                    "this is always present",
                                    "`if let` takes the value out of something that may be \
                                     empty; for a `bool`, write a plain `if` (spec §9.4)",
                                );
                                Ty::Error
                            }
                        };
                        self.push();
                        let local = self.bind(
                            &b.name.name,
                            bound,
                            b.mutable,
                            LocalKind::Binding,
                            b.name.span,
                        );
                        self.if_let_bindings.push(local);
                        let then = self.block_valued(&i.then, expected, wanted);
                        self.pop();
                        (Ok((local, subject.id)), then)
                    }
                };
                let (ty, else_branch) = match &i.else_branch {
                    Some(else_expr) => {
                        // Handed on to the `else` block or `else if`, which read it on entry.
                        self.stmt_position = !wanted;
                        let other = self.expr(else_expr, expected);
                        // The parser only ever produces a block or another `if` here, and
                        // both are single expressions, so the branch is one HIR block.
                        let block = self.hir.block(else_expr.span, vec![]);
                        let stmt = self.hir.stmt(else_expr.span, hir::StmtKind::Expr(other.id));
                        self.hir.set_block_stmts(block, vec![stmt]);
                        (join(&then.ty, &other.ty).unwrap_or(Ty::Void), Some(block))
                    }
                    // Without an `else` an `if` cannot produce a value.
                    None => (Ty::Void, None),
                };
                let kind = match head {
                    Err(cond) => hir::ExprKind::If { cond, then: then.id, else_branch },
                    Ok((binding, operand)) => {
                        hir::ExprKind::IfLet { binding, operand, then: then.id, else_branch }
                    }
                };
                let c = self.mk(e, ty, kind);
                self.settle_pending_nones(wanted, c, e.span)
            }
            ExprKind::Switch(s) => self.switch(s, e, expected),
            ExprKind::Or { expr, clause } => self.or_expr(expr, clause, e, expected),
            ExprKind::Return(v) => {
                self.refuse_return_in_base_init(e.span);
                let ret = self.ret.clone();
                let mut value = None;
                match v {
                    Some(expr) => {
                        // `return .Default{}` from a `-> ButtonState?` returns the success
                        // case, so the leading dot infers from the `T`, not the `T?` (spec
                        // §7.4, §9.1).
                        let want = match (&ret, &expr.kind) {
                            (Ty::Optional(inner), ExprKind::DotInfer { .. }) => (**inner).clone(),
                            _ => ret.clone(),
                        };
                        let c = self.expr(expr, Some(&want));
                        value = Some(c.id);
                        let t = c.ty;
                        // `return error(...)` is the failure case of a `!->` function
                        // (spec §9.1); its payload is `Never`, so it unifies with any
                        // success type and only the signature has to be checked.
                        if let Ty::Fallible(_) = &t {
                            if !self.fallible {
                                self.cx.error_help(
                                    codes::TRY_OUTSIDE_FALLIBLE,
                                    expr.span,
                                    "this function cannot fail",
                                    "there is nowhere for this error to go",
                                    "mark the signature fallible by writing `!->` in place of `->` (spec §9.1)",
                                );
                            }
                        } else {
                            // Returning a plain `T` from a `T?` function is the success
                            // case — there is no wrapping step (spec §9.1).
                            let target = match &ret {
                                Ty::Optional(inner) if !assignable(&ret, &t) => (**inner).clone(),
                                other => other.clone(),
                            };
                            self.expect(&target, &t, expr.span);
                            // `return [1, 2, 3]` from a `-> []i32` is the same certain
                            // dangle as storing one (§5.3). Returning a slice of a *local*
                            // array dangles too, but that needs escape analysis to see,
                            // and §1.1 leaves it to the programmer as with a stale `^T`.
                            self.reject_temporary_slice(&target, expr, &t);
                        }
                    }
                    None => {
                        if !matches!(ret, Ty::Void | Ty::Error) {
                            let n = self.cx.name_of(&ret);
                            self.cx.error(
                                codes::TYPE_MISMATCH,
                                e.span,
                                format!("this function returns `{n}`"),
                                "`return` needs a value here",
                            );
                        }
                    }
                }
                self.mk(e, Ty::Never, hir::ExprKind::Return(value))
            }
            ExprKind::Break { label } => {
                let target = self.loop_jump(label.as_ref(), e.span, true);
                self.mk(e, Ty::Never, hir::ExprKind::Break(target))
            }
            ExprKind::Continue { label } => {
                let target = self.loop_jump(label.as_ref(), e.span, false);
                self.mk(e, Ty::Never, hir::ExprKind::Continue(target))
            }
            ExprKind::Intrinsic { name, args } => {
                let args = args.iter().map(|a| self.expr(a, None).id).collect();
                // spec §12.7
                let ty = match name.name.as_str() {
                    "line" | "column" => Ty::Int(IntTy::U32),
                    "file" | "function" | "module" | "expr_text" => Ty::Str,
                    "location" => Ty::Builtin(dz_hir::ty::BuiltinTy::SourceLocation),
                    _ => Ty::Error,
                };
                let kind = hir::ExprKind::Intrinsic { name: name.name.clone(), args };
                self.mk(e, ty, kind)
            }
            // Macro bodies are opaque until M10.
            ExprKind::MacroCall { .. } | ExprKind::Error => {
                self.mk(e, Ty::Error, hir::ExprKind::Error)
            }
        }
    }

    /// Push the node the checker just decided on, tagged with its source expression.
    fn mk(&mut self, e: &Expr, ty: Ty, kind: hir::ExprKind) -> Checked {
        self.hir.expr(e.span, ty, kind)
    }

    // --- sub-checks -------------------------------------------------------

    fn binary(
        &mut self,
        op: BinaryOp,
        lhs: &Expr,
        rhs: &Expr,
        e: &Expr,
        expected: Option<&Ty>,
    ) -> Checked {
        use BinaryOp::*;
        let span = e.span;
        let (ty, a, b) = match op {
            And | Or => {
                let a = self.expr(lhs, Some(&Ty::Bool));
                let b = self.expr(rhs, Some(&Ty::Bool));
                self.require_bool(&a.ty, lhs.span, op.as_str());
                self.require_bool(&b.ty, rhs.span, op.as_str());
                (Ty::Bool, a, b)
            }
            Eq | Ne | Lt | Gt | Le | Ge => {
                // Whichever side can stand alone is checked first, so it can supply the
                // expected type for the other. That is what makes `.Left == s` work as
                // well as `s == .Left` (spec §7.4) — a comparison has no reason to prefer
                // one side, and without this the left operand of every comparison would be
                // the one place a variant could not be written with a leading dot.
                let (a, b) = if needs_context(lhs) && !needs_context(rhs) {
                    let b = self.expr(rhs, None);
                    let a = self.expr(lhs, Some(&b.ty));
                    (a, b)
                } else {
                    let a = self.expr(lhs, None);
                    let b = self.expr(rhs, Some(&a.ty));
                    (a, b)
                };
                if join(&a.ty, &b.ty).is_none() {
                    let (x, y) = (self.cx.name_of(&a.ty), self.cx.name_of(&b.ty));
                    self.cx.error_help(
                        codes::INVALID_OPERANDS,
                        span,
                        format!("cannot compare `{x}` with `{y}`"),
                        "operands have different types",
                        "Dazzle has no implicit conversions (spec §5.1) — cast one side",
                    );
                    return self.mk(e, Ty::Bool, hir::ExprKind::Error);
                }
                // A `T?` is compared only with `none`, which asks whether it is empty (spec
                // §9). Two optionals have no comparison in the spec, and used to reach C++
                // as `Opt == Opt`, which does not compile (invariant 7).
                let is_none = |x: &Expr| matches!(x.kind, ExprKind::None);
                let optional = matches!(a.ty, Ty::Optional(_)) || matches!(b.ty, Ty::Optional(_));
                if optional && !(matches!(op, Eq | Ne) && (is_none(lhs) || is_none(rhs))) {
                    let (x, y) = (self.cx.name_of(&a.ty), self.cx.name_of(&b.ty));
                    self.cx.error_help(
                        codes::INVALID_OPERANDS,
                        span,
                        format!("cannot compare `{x}` with `{y}`"),
                        "an optional is compared only with `none`",
                        "`x == none` asks whether it is empty; to use the value, take it out \
                         with `or` first (spec §9.4)",
                    );
                    return self.mk(e, Ty::Bool, hir::ExprKind::Error);
                }
                // `h == null` on a handle asks the handle, which for a `Weak` also means
                // "what it referred to is gone" (spec §14.5). Nothing else compares one with
                // `null`; two handles compare only through `Eq`, like any struct.
                let is_null = |x: &Expr| matches!(x.kind, ExprKind::Null);
                if matches!(op, Eq | Ne) && (is_null(lhs) || is_null(rhs)) {
                    let (handle, handle_ast) = if is_null(rhs) { (&a, lhs) } else { (&b, rhs) };
                    let handle_ty = handle.ty.peeled_refs().clone();
                    if let Some(r) = self.nullable(&handle_ty, handle_ast.span) {
                        let test = self.nullable_call(&r, handle.id, &handle_ty, "is_null", span);
                        if op == Eq {
                            return test;
                        }
                        let kind = hir::ExprKind::Unary { op: UnaryOp::Not, operand: test.id };
                        return self.mk(e, Ty::Bool, kind);
                    }
                }
                // A user type is compared through `Eq` or `Ord`, not by the machine
                // (spec §11.2). Without this the comparison would reach C++ and fail there
                // for a struct with no `operator==`, which invariant 7 forbids. Only through
                // references: two pointers compare as addresses, which is what `p != null`
                // asks, whatever they point at (spec §5.2).
                if self.needs_operator_trait(a.ty.peeled_refs()) {
                    return self.comparison_through_trait(op, &a, &b, rhs, e);
                }
                (Ty::Bool, a, b)
            }
            BitAnd | BitOr | BitXor | Shl | Shr => {
                let a = self.expr(lhs, expected);
                let b = self.expr(rhs, Some(&a.ty));
                let mut ty = None;
                for (t, at) in [(&a.ty, lhs), (&b.ty, rhs)] {
                    if !t.is_wildcard() && !t.is_integral() {
                        let n = self.cx.name_of(t);
                        let o = op.as_str();
                        self.cx.error(
                            codes::INVALID_OPERANDS,
                            at.span,
                            format!("`{o}` needs integer operands, found `{n}`"),
                            "not an integer",
                        );
                        ty = Some(Ty::Error);
                        break;
                    }
                }
                let ty = ty.unwrap_or_else(|| join(&a.ty, &b.ty).unwrap_or(Ty::Error));
                (ty, a, b)
            }
            Add | Sub | Mul | Div | Rem => {
                let a = self.expr(lhs, expected);
                // `v1 + v2` and `v * scalar` on a user type go through the trait §11.2
                // names for the operator (spec §11.2).
                if self.needs_operator_trait(a.ty.peeled()) {
                    let (trait_name, method) = match op {
                        Add => ("Add", "add"),
                        Sub => ("Sub", "sub"),
                        Mul => ("Mul", "mul"),
                        Div => ("Div", "div"),
                        _ => ("Rem", "rem"),
                    };
                    // The right operand is *not* given the left's type as context: `v *
                    // scalar` is what §11.2's `Rhs` parameter exists for, so the sides may
                    // differ. It is checked twice — once without context to see which
                    // implementation applies, then against that implementation's `Rhs` so an
                    // untyped literal takes the right type. `3` in `v * 3` has to become an
                    // `f32` here, not an `i32` that C++ would quietly convert.
                    let probe = self.expr_quiet(rhs);
                    let want = self.operator_rhs_type(trait_name, method, &a.ty, &probe);
                    let b = self.expr(rhs, want.as_ref());
                    let call =
                        self.operator_call(trait_name, method, &a, Some(&b), Some(rhs), span);
                    return match call {
                        Some(c) => c,
                        None => {
                            let t = a.ty.peeled().clone();
                            self.operator_needs_trait(op.as_str(), &t, trait_name, span);
                            self.mk(e, Ty::Error, hir::ExprKind::Error)
                        }
                    };
                }
                let b = self.expr(rhs, Some(&a.ty));
                let mut ty = None;
                for (t, at) in [(&a.ty, lhs), (&b.ty, rhs)] {
                    if !t.is_wildcard() && !t.is_numeric() {
                        let n = self.cx.name_of(t);
                        let o = op.as_str();
                        self.cx.error(
                            codes::INVALID_OPERANDS,
                            at.span,
                            format!("`{o}` cannot be applied to `{n}`"),
                            "not a numeric type",
                        );
                        ty = Some(Ty::Error);
                        break;
                    }
                }
                let ty = match ty {
                    Some(t) => t,
                    None => match join(&a.ty, &b.ty) {
                        Some(t) => t,
                        None => {
                            let (x, y) = (self.cx.name_of(&a.ty), self.cx.name_of(&b.ty));
                            let o = op.as_str();
                            self.cx.error_help(
                                codes::INVALID_OPERANDS,
                                span,
                                format!("`{o}` cannot mix `{x}` and `{y}`"),
                                "operands have different types",
                                "Dazzle has no implicit numeric conversions, not even widening \
                                 (spec §5.1) — cast one side with `as`",
                            );
                            Ty::Error
                        }
                    },
                };
                (ty, a, b)
            }
        };
        self.mk(e, ty, hir::ExprKind::Binary { op, lhs: a.id, rhs: b.id })
    }

    fn check_cast(&mut self, from: &Ty, to: &Ty, span: Span) {
        if from.is_wildcard() || to.is_wildcard() {
            return;
        }
        let ok = match (from.peeled(), to) {
            // Numbers convert to numbers, explicitly and possibly lossily.
            (a, b) if a.is_numeric() && b.is_numeric() => true,
            (Ty::Char, b) if b.is_integral() => true,
            (a, Ty::Char) if a.is_integral() => true,
            (Ty::Bool, b) if b.is_integral() => true,
            // A pointer's address, and — inside `unsafe` — an address back to a pointer
            // (spec §10.6, 18.49). Checked on `from` itself: `peeled` would see through it.
            _ if matches!(from, Ty::Ptr { .. } | Ty::OwnPtr { .. })
                && matches!(to, Ty::Int(IntTy::Usize)) =>
            {
                true
            }
            _ if matches!(from, Ty::Int(IntTy::Usize) | Ty::IntLit)
                && matches!(to, Ty::Ptr { .. }) =>
            {
                self.require_unsafe(span, "making a pointer from an address");
                true
            }
            // An enum converts to its backing type (spec §7.4).
            (Ty::Named { sym: id, .. }, b) if b.is_integral() => {
                matches!(self.cx.table.get(*id), Some(ItemSig::Enum { .. }))
            }
            _ => matches!(
                (from, to),
                (Ty::Ptr { .. } | Ty::OwnPtr { .. }, Ty::Ptr { .. })
                    // `"x" as cstr`: the literal already became one from the `as`.
                    | (Ty::CStr, Ty::CStr)
                    // Measuring a C string costs nothing but a `strlen` (spec §5.3).
                    | (Ty::CStr, Ty::Str)
            ),
        };
        // A string's bytes, viewed; and bytes or a pointer taken as text, which nothing
        // checks is UTF-8 or terminated — `str::from_utf8` is the checked way (spec §5.3).
        let u8_slice =
            |t: &Ty| matches!(t, Ty::Slice { elem, .. } if matches!(**elem, Ty::Int(IntTy::U8)));
        let u8_ptr = |t: &Ty| matches!(t, Ty::Ptr { inner, .. } | Ty::OwnPtr { inner, .. } if matches!(**inner, Ty::Int(IntTy::U8)));
        let ok = ok
            || match (from, to) {
                (Ty::Str, t) if u8_slice(t) && !matches!(t, Ty::Slice { mutable: true, .. }) => {
                    true
                }
                (f, Ty::Str) if u8_slice(f) => {
                    self.require_unsafe(span, "taking bytes as a `str` without checking them");
                    true
                }
                (f, Ty::CStr) if u8_ptr(f) => {
                    self.require_unsafe(span, "taking a pointer as a `cstr`");
                    true
                }
                _ => false,
            };
        if !ok && matches!((from, to), (Ty::Str, Ty::CStr)) {
            self.cx.error_help(
                codes::INVALID_CAST,
                span,
                "a runtime `str` cannot become a `cstr` yet",
                "not a string literal",
                "a `str` has no zero after its last byte, so a C string needs a copy of it — \
                 an owning one, which arrives with the stdlib's `String` (spec §5.3, §19 M12). \
                 A string literal is a `cstr` as it is",
            );
            return;
        }
        if !ok {
            let (a, b) = (self.cx.name_of(from), self.cx.name_of(to));
            self.cx.error_help(
                codes::INVALID_CAST,
                span,
                format!("cannot cast `{a}` to `{b}`"),
                "invalid conversion",
                "`as` converts between numbers, between pointers, and from an enum to its \
                 backing type; it never runs user code (spec §10.6)",
            );
        }
    }

    fn field_type(&mut self, base: &Ty, name: &Ident, base_span: Span) -> (Ty, Option<usize>) {
        let peeled = base.peeled().clone();
        // Tuple access: `t.0`
        if let Ty::Tuple(parts) = &peeled {
            if let Ok(i) = name.name.parse::<usize>() {
                return match parts.get(i) {
                    Some(t) => (t.clone(), Some(i)),
                    None => {
                        let n = parts.len();
                        self.cx.error(
                            codes::NO_SUCH_FIELD,
                            name.span,
                            format!("this tuple has {n} elements"),
                            format!("no element `{i}`"),
                        );
                        (Ty::Error, None)
                    }
                };
            }
        }
        // Arrays and slices both carry a length, and hand out their first element as a
        // pointer for unchecked access (spec §5.3).
        if let Ty::Array { elem, .. } | Ty::Slice { elem, .. } = &peeled {
            if name.name == "len" {
                return (Ty::Int(IntTy::Usize), None);
            }
            // Writable or not depends on the place, which the caller sees and this does not.
            if name.name == "ptr" {
                return (Ty::ptr(false, (**elem).clone()), None);
            }
        }
        // A `str` is `{ ptr, len }` (spec §5.3): its length in bytes, and its first byte.
        if matches!(peeled, Ty::Str) {
            if name.name == "len" {
                return (Ty::Int(IntTy::Usize), None);
            }
            if name.name == "ptr" {
                return (Ty::ptr(false, Ty::Int(IntTy::U8)), None);
            }
        }
        if peeled.is_wildcard() {
            return (Ty::Error, None);
        }
        // `Error` and `SourceLocation` have no declaration to look a field up in, so
        // their field lists come from the type itself (spec §9.2, §12.7).
        if let Ty::Builtin(b) = &peeled {
            let fields = b.fields();
            return match fields.iter().position(|(n, _)| *n == name.name) {
                Some(i) => (fields[i].1.clone(), Some(i)),
                None => {
                    let owner = b.name();
                    let names: Vec<&str> = fields.iter().map(|(n, _)| *n).collect();
                    let suggestion = dz_resolve::closest(&name.name, names.into_iter());
                    let mut d = dz_diag::Diagnostic::error(
                        codes::NO_SUCH_FIELD,
                        format!("`{owner}` has no field `{}`", name.name),
                    )
                    .at(self.cx.file, name.span, "unknown field");
                    if let Some(s) = suggestion {
                        d = d.with_help(format!("did you mean `{s}`?"));
                    }
                    self.cx.diags.push(d);
                    (Ty::Error, None)
                }
            };
        }
        // A `case Circle(c)` binding: `c` is the variant's own field list (spec §7.5).
        if let Ty::Variant(owner, index, args) = &peeled {
            let variant = match self.cx.table.get(*owner) {
                Some(ItemSig::Union { variants }) => variants.get(*index).cloned(),
                _ => None,
            };
            let Some(variant) = variant else { return (Ty::Error, None) };
            let found = variant.fields.iter().enumerate().find(|(_, f)| f.name == name.name);
            return match found {
                // Read through the union's arguments: `value: T` on a `Maybe<i32>` is `i32`.
                Some((i, _)) => {
                    let types = self.cx.table.field_types(*owner, args, &variant.fields);
                    (types[i].clone(), Some(i))
                }
                None => {
                    let owner_name = self.cx.res.symbol(*owner).name.clone();
                    let names: Vec<String> =
                        variant.fields.iter().map(|f| f.name.clone()).collect();
                    let suggestion =
                        dz_resolve::closest(&name.name, names.iter().map(String::as_str));
                    let mut d = dz_diag::Diagnostic::error(
                        codes::NO_SUCH_FIELD,
                        format!("`{owner_name}::{}` has no field `{}`", variant.name, name.name),
                    )
                    .at(self.cx.file, name.span, "unknown field");
                    if let Some(s) = suggestion {
                        d = d.with_help(format!("did you mean `{s}`?"));
                    }
                    self.cx.diags.push(d);
                    (Ty::Error, None)
                }
            };
        }
        // A generic parameter has no fields of its own; what it has is what its bounds
        // give it, and a trait declares methods and associated types, never fields
        // (spec §7.6, §8.1).
        if let Ty::Param(p) = &peeled {
            self.no_such_member_on_param(p, name, "field");
            return (Ty::Error, None);
        }
        let Some(sym) = peeled.named() else {
            let t = self.cx.name_of(&peeled);
            self.cx.error(
                codes::NO_SUCH_FIELD,
                name.span,
                format!("`{t}` has no field `{}`", name.name),
                "not a struct",
            );
            return (Ty::Error, None);
        };
        // A field of an instantiation has the instantiated type: `Buffer<f32>.m_data` is
        // `^f32`, from the `^T` the declaration wrote (spec §8.4).
        let args: Vec<Ty> = match &peeled {
            Ty::Named { args, .. } => args.clone(),
            _ => Vec::new(),
        };
        let Some(sig) = self.cx.table.struct_sig(sym) else {
            let t = self.cx.name_of(&peeled);
            self.cx.error(
                codes::NO_SUCH_FIELD,
                name.span,
                format!("`{t}` has no fields"),
                "not a struct",
            );
            return (Ty::Error, None);
        };
        let found = sig.fields.iter().enumerate().find(|(_, f)| f.name == name.name);
        let Some((index, field)) = found.map(|(i, f)| (i, f.clone())) else {
            let owner = self.cx.res.symbol(sym).name.clone();
            let names: Vec<String> = sig.fields.iter().map(|f| f.name.clone()).collect();
            let has_method = sig.methods.contains_key(&name.name);
            let suggestion = dz_resolve::closest(&name.name, names.iter().map(String::as_str));
            let mut d = dz_diag::Diagnostic::error(
                codes::NO_SUCH_FIELD,
                format!("`{owner}` has no field `{}`", name.name),
            )
            .at(self.cx.file, name.span, "unknown field");
            if has_method {
                d = d.with_help(format!(
                    "`{}` is a method — call it as `.{}()`",
                    name.name, name.name
                ));
            } else if let Some(s) = suggestion {
                d = d.with_help(format!("did you mean `{s}`?"));
            }
            self.cx.diags.push(d);
            let _ = base_span;
            return (Ty::Error, None);
        };
        self.check_member_visible(sym, &field.name, field.vis, field.span, name.span, "field");
        (self.cx.instantiate(sym, &args, &field.ty), Some(index))
    }

    // --- operators through traits (spec §11.2) -----------------------------

    /// Does this type need a trait to be compared or added, rather than being something the
    /// language handles itself?
    ///
    /// Everything except a user struct or union, and a generic parameter, keeps the built-in
    /// operator it has always had: integers, floats, `bool`, `char`, pointers, enums (which
    /// are named integers, §7.4) and slices are compared by the machine.
    fn needs_operator_trait(&self, t: &Ty) -> bool {
        match t {
            Ty::Param(_) => true,
            Ty::Named { sym, .. } => matches!(
                self.cx.table.get(*sym),
                Some(ItemSig::Struct(_)) | Some(ItemSig::Union { .. })
            ),
            _ => false,
        }
    }

    /// The `Rhs` of whichever implementation of `trait_name` accepts `arg` (spec §11.2).
    ///
    /// Used as the expected type when the right operand is checked for real, so an untyped
    /// literal settles on the implementation's type rather than on its own default.
    fn operator_rhs_type(
        &mut self,
        trait_name: &str,
        method: &str,
        recv: &Ty,
        arg: &Ty,
    ) -> Option<Ty> {
        // A probe: the real call reports an ambiguity, so this one must not as well — and
        // an ambiguous operand is left untyped, or the real call would see it already
        // settled on the first implementation and have nothing to report.
        let before = self.cx.diags.len();
        let found = self.select_operator_impl(trait_name, method, recv, Some(arg), Span::DUMMY);
        let ambiguous = self.cx.diags.len() > before;
        self.cx.diags.truncate(before);
        if ambiguous {
            return None;
        }
        let (_, sig) = found?;
        sig.params.first().map(|p| p.ty.clone())
    }

    /// Which implementation of an operator's trait applies to this pair of operands.
    ///
    /// A type may implement `Mul<f32>` and `Mul<Vec2>` both, so the applicable one is the
    /// one whose `Rhs` accepts the right operand — not one built from the operand's type,
    /// which would miss `v * 3` where the literal is still untyped (spec §11.2).
    fn select_operator_impl(
        &mut self,
        trait_name: &str,
        method: &str,
        recv: &Ty,
        arg: Option<&Ty>,
        span: Span,
    ) -> Option<(TraitRef, FnSig)> {
        let trait_sym = self.cx.prelude_trait(trait_name)?;
        let self_ty = recv.peeled().clone();
        let declared = self.declared_traits_of(&self_ty);
        let found: Vec<(TraitRef, FnSig)> = self
            .cx
            .table
            .trait_closure(&declared, &self_ty)
            .into_iter()
            .filter(|r| r.trait_sym == trait_sym)
            .filter_map(|r| {
                let sig = self.cx.table.trait_method(&r, &self_ty, method)?;
                Some((r, sig))
            })
            .collect();
        if found.is_empty() {
            return None;
        }
        // An implementation that does not accept this operand is still returned when it is
        // the only one: the argument check then names both types, the better diagnostic.
        let sigs: Vec<FnSig> = found.iter().map(|(_, s)| s.clone()).collect();
        let i = self.pick_overload(&sigs, arg, span, method);
        found.into_iter().nth(i)
    }

    /// A method by name, read through `sym`'s instantiation — and when `sym` has several of
    /// that name, the one whose first parameter the first argument fits (spec §18.27).
    fn method_by_args(
        &mut self,
        sym: SymbolId,
        type_args: &[Ty],
        name: &str,
        args: &[Expr],
        span: Span,
    ) -> Option<FnSig> {
        let keys = self.cx.table.struct_sig(sym)?.overloads.get(name)?.clone();
        // A method of `Buffer<f32>` has `f32` wherever it wrote `T`.
        let sigs: Vec<FnSig> = keys
            .iter()
            .filter_map(|k| self.cx.table.struct_sig(sym)?.methods.get(k).cloned())
            .map(|s| self.cx.instantiate_sig(sym, type_args, &s))
            .collect();
        if sigs.len() <= 1 {
            return sigs.into_iter().next();
        }
        let arg = args.first().map(|a| self.expr_quiet(a));
        let i = self.pick_overload(&sigs, arg.as_ref(), span, name);
        sigs.into_iter().nth(i)
    }

    /// Which of several same-named methods a first argument picks (spec §18.27): the one
    /// its type fits, preferring an exact match and then the type an untyped literal would
    /// default to. A literal that fits several and defaults to none of them is ambiguous.
    /// With nothing fitting, the first is returned and the argument check reports.
    fn pick_overload(&mut self, sigs: &[FnSig], arg: Option<&Ty>, span: Span, name: &str) -> usize {
        let Some(arg) = arg else { return 0 };
        let first = |s: &FnSig| s.params.first().map(|p| p.ty.peeled().clone());
        let fits: Vec<usize> = (0..sigs.len())
            .filter(|&i| first(&sigs[i]).is_some_and(|p| assignable(&p, arg.peeled())))
            .collect();
        if fits.len() <= 1 {
            return fits.first().copied().unwrap_or(0);
        }
        for want in [arg.peeled().clone(), arg.defaulted()] {
            let exact: Vec<usize> =
                fits.iter().copied().filter(|&i| first(&sigs[i]).as_ref() == Some(&want)).collect();
            if exact.len() == 1 {
                return exact[0];
            }
        }
        let shown = self.cx.name_of(arg);
        self.cx.error_help(
            codes::AMBIGUOUS_CALL,
            span,
            format!("more than one `{name}` accepts `{shown}`"),
            "ambiguous call",
            "give the argument a type — `3 as i64`, say — so that exactly one fits \
             (spec §18.27)",
        );
        fits[0]
    }

    /// Find the trait method an operator dispatches to, and build the call (spec §11.2).
    ///
    /// `None` means the operand does not implement the trait; the caller reports that,
    /// because only it knows which operator was written.
    fn operator_call(
        &mut self,
        trait_name: &str,
        method: &str,
        recv: &Checked,
        arg: Option<&Checked>,
        arg_expr: Option<&Expr>,
        span: Span,
    ) -> Option<Checked> {
        let arg_ty = arg.map(|a| a.ty.clone());
        let (r, sig) =
            self.select_operator_impl(trait_name, method, &recv.ty, arg_ty.as_ref(), span)?;
        let trait_sym = r.trait_sym;

        let mut arg_ids = Vec::new();
        if let (Some(a), Some(e)) = (arg, arg_expr) {
            let want = sig.params.first().map(|p| p.ty.clone()).unwrap_or(Ty::Error);
            // `Eq::eq` takes `&Self`, so borrow the operand rather than passing it —
            // otherwise a move-only type would be consumed by `==`. A temporary has nothing
            // to borrow and nothing to consume, so it goes by value and C++ binds it to the
            // `const T&` parameter itself.
            let id = match (&want, self.place_mutability(e).is_some()) {
                // `a == Vec3{…}`: a temporary goes by value, bound by C++ to the `const T&`.
                (Ty::Ref { mutable: false, inner }, false) if assignable(inner, &a.ty) => a.id,
                (Ty::Ref { mutable: false, .. }, true) => {
                    self.hir
                        .expr(
                            e.span,
                            Ty::reference(false, a.ty.clone()),
                            hir::ExprKind::Ref { mutable: false, place: a.id },
                        )
                        .id
                }
                _ => {
                    self.expect(&want, &a.ty, e.span);
                    a.id
                }
            };
            arg_ids.push(id);
        }

        let callee = hir::Callee::Method {
            recv: recv.id,
            owner: trait_sym,
            name: method.to_owned(),
            method_args: Vec::new(),
        };
        let node =
            self.hir.expr(span, sig.ret.clone(), hir::ExprKind::Call { callee, args: arg_ids });
        Some(node)
    }

    /// `a == b` and `a < b` on a user type (spec §11.2).
    ///
    /// `==`/`!=` go to `Eq::eq`. The four orderings go to `Ord::cmp` and then compare its
    /// `Ordering` against the variant that answers the question — so a type implements one
    /// method and gets four operators, and `<` and `>=` cannot disagree with each other.
    fn comparison_through_trait(
        &mut self,
        op: BinaryOp,
        a: &Checked,
        b: &Checked,
        rhs: &Expr,
        e: &Expr,
    ) -> Checked {
        use BinaryOp::*;
        let equality = matches!(op, Eq | Ne);
        let (trait_name, method) = if equality { ("Eq", "eq") } else { ("Ord", "cmp") };
        let Some(call) = self.operator_call(trait_name, method, a, Some(b), Some(rhs), e.span)
        else {
            self.operator_needs_trait(op.as_str(), a.ty.peeled(), trait_name, e.span);
            return self.mk(e, Ty::Bool, hir::ExprKind::Error);
        };

        if equality {
            // `!=` is `!eq(...)`; there is no separate `ne` to get out of step with `eq`.
            return match op {
                Ne => self.mk(
                    e,
                    Ty::Bool,
                    hir::ExprKind::Unary { op: UnaryOp::Not, operand: call.id },
                ),
                _ => call,
            };
        }

        // `cmp` yields an `Ordering`; each operator is one comparison against one variant.
        // `<` and `>` test for the variant, `<=` and `>=` test for its opposite.
        let (variant, negate) = match op {
            Lt => ("Less", false),
            Gt => ("Greater", false),
            Le => ("Greater", true),
            _ => ("Less", true),
        };
        let Some(ordering) = self.cx.prelude_enum("Ordering") else {
            return self.mk(e, Ty::Bool, hir::ExprKind::Error);
        };
        let index = match self.cx.table.get(ordering) {
            Some(ItemSig::Enum { variants, .. }) => variants.iter().position(|v| v.name == variant),
            _ => None,
        };
        let Some(index) = index else {
            return self.mk(e, Ty::Bool, hir::ExprKind::Error);
        };
        let want = self.hir.expr(
            e.span,
            Ty::plain(ordering),
            hir::ExprKind::EnumVariant { owner: ordering, index },
        );
        self.mk(
            e,
            Ty::Bool,
            hir::ExprKind::Binary { op: if negate { Ne } else { Eq }, lhs: call.id, rhs: want.id },
        )
    }

    /// An operator its operand's type does not provide (spec §11.2).
    fn operator_needs_trait(&mut self, op: &str, t: &Ty, trait_name: &str, span: Span) {
        let shown = self.cx.name_of(t);
        self.cx.error_help(
            codes::UNSATISFIED_BOUND,
            span,
            format!("`{op}` cannot be applied to `{shown}`"),
            format!("`{shown}` does not implement `{trait_name}`"),
            format!(
                "operators are traits (spec §11.2) — implement it: `impl {trait_name} for \
                 {shown} {{ … }}`"
            ),
        );
    }

    // --- calling a generic (spec §8.1, §8.4) -------------------------------

    /// Work out a generic call's type arguments and return the concrete signature.
    ///
    /// Arguments may be written (`largest<Score>(a, b)`) or inferred from the argument
    /// types. Inference is deliberately simple — one pass, structural, no backtracking —
    /// because definition-site checking has already done the hard part: the body was
    /// verified against the bounds, so all that is left here is to pick the types and check
    /// they satisfy those bounds.
    fn solve_call(
        &mut self,
        sig: &FnSig,
        seg: &PathSegment,
        args: &[Expr],
        span: Span,
        name: &str,
    ) -> (FnSig, Vec<Ty>) {
        let mut map: HashMap<u32, Ty> = HashMap::new();

        if !seg.generics.is_empty() {
            let written: Vec<Ty> = seg.generics.iter().map(|t| self.cx.lower_type(t)).collect();
            if written.len() != sig.generics.len() {
                let (wanted, got) = (sig.generics.len(), written.len());
                self.cx.error_help(
                    codes::WRONG_TYPE_ARG_COUNT,
                    seg.span,
                    format!(
                        "`{name}` takes {wanted} type {}, but {got} {} given",
                        plural(wanted, "argument"),
                        if got == 1 { "was" } else { "were" }
                    ),
                    format!("expected {wanted}, found {got}"),
                    format!(
                        "`{name}` is declared with {wanted} type {}",
                        plural(wanted, "parameter")
                    ),
                );
            }
            for (def, ty) in sig.generics.iter().zip(written) {
                map.insert(def.param.id, ty);
            }
        } else {
            // Each argument is checked once, without an expected type, and its type is
            // matched against the parameter's to solve for the parameters it mentions.
            for (a, p) in args.iter().zip(&sig.params) {
                if !p.ty.has_params() {
                    continue;
                }
                let actual = self.expr_quiet(a);
                unify(&p.ty, &actual.defaulted(), &mut map);
            }
        }

        // A parameter no argument mentions cannot be inferred. Saying so here is much
        // better than the cascade of mismatches that would follow.
        for def in &sig.generics {
            if let std::collections::hash_map::Entry::Vacant(slot) = map.entry(def.param.id) {
                slot.insert(Ty::Error);
                let t = def.name();
                self.cx.error_help(
                    codes::WRONG_TYPE_ARG_COUNT,
                    span,
                    format!("cannot infer `{t}` in this call to `{name}`"),
                    format!("`{t}` is not determined by any argument"),
                    format!("write it out: `{name}<{t}>(…)` (spec §8.1)"),
                );
            }
        }

        for def in &sig.generics {
            let chosen = map[&def.param.id].clone();
            self.check_bounds(def, &chosen, span);
        }

        let solved: Vec<Ty> = sig.generics.iter().map(|d| map[&d.param.id].clone()).collect();
        let concrete = FnSig {
            params: sig
                .params
                .iter()
                .map(|p| crate::ParamSig {
                    name: p.name.clone(),
                    ty: ty::substitute(&p.ty, &map),
                    has_default: p.has_default,
                    caller_default: p.caller_default.clone(),
                    c_cast: p.c_cast.clone(),
                })
                .collect(),
            ret: ty::substitute(&sig.ret, &map),
            generics: Vec::new(),
            ..sig.clone()
        };
        (concrete, solved)
    }

    /// `n.hash<u8>()`: type arguments written on a method that can have none — a trait's,
    /// a bound's, or a number's (spec §8.1, DZ0557 refuses them in declarations).
    fn no_method_type_args(&mut self, generics: &[Type], name: &Ident) {
        let (Some(first), Some(last)) = (generics.first(), generics.last()) else { return };
        self.cx.error_help(
            codes::WRONG_TYPE_ARG_COUNT,
            Span { start: first.span.start, end: last.span.end },
            format!("`{}` takes no type arguments", name.name),
            "not a generic method",
            "a trait's method has no type parameters of its own; remove `<…>` (spec §8.1)",
        );
    }

    /// A generic function taken as a value — `sort_by(less<i32>)`, `let f: fn(i32) -> i32 =
    /// twice` — instantiated there: its arguments written after the name, or read off the
    /// function type the context wants (spec §8.1). Empty for a function that is not
    /// generic.
    fn fn_value_type_args(
        &mut self,
        sig: &FnSig,
        seg: &PathSegment,
        expected: Option<&Ty>,
        span: Span,
    ) -> Vec<Ty> {
        if !sig.is_generic() {
            return Vec::new();
        }
        let name = seg.name.name.clone();
        let mut map: HashMap<u32, Ty> = HashMap::new();
        if !seg.generics.is_empty() {
            let written: Vec<Ty> = seg.generics.iter().map(|t| self.cx.lower_type(t)).collect();
            if written.len() != sig.generics.len() {
                let (wanted, got) = (sig.generics.len(), written.len());
                self.cx.error(
                    codes::WRONG_TYPE_ARG_COUNT,
                    seg.span,
                    format!(
                        "`{name}` takes {wanted} type {}, found {got}",
                        plural(wanted, "argument")
                    ),
                    format!("expected {wanted}"),
                );
            }
            for (def, t) in sig.generics.iter().zip(written) {
                map.insert(def.param.id, t);
            }
        } else if let Some(Ty::Fn { params, ret }) = expected.map(|t| self.subst(t)) {
            for (p, a) in sig.params.iter().zip(params.iter()) {
                unify(&p.ty, a, &mut map);
            }
            unify(&sig.ret, &ret, &mut map);
        }
        let mut args = Vec::with_capacity(sig.generics.len());
        for def in &sig.generics {
            let chosen = match map.get(&def.param.id) {
                Some(t) => t.clone(),
                None => {
                    let t = def.name();
                    self.cx.error_help(
                        codes::WRONG_TYPE_ARG_COUNT,
                        span,
                        format!("cannot infer `{t}` for `{name}` used as a value"),
                        format!("`{t}` is not determined here"),
                        format!("write it out: `{name}<{t}>` (spec §8.1)"),
                    );
                    Ty::Error
                }
            };
            self.check_bounds(def, &chosen, span);
            args.push(chosen);
        }
        args
    }

    /// A method's own type parameters, `fn map<U>(&self, …)`, solved at the call exactly as
    /// a generic function's are: written (`b.map<f32>(…)`, `Bag::pick<u8>(…)`) or inferred
    /// from the arguments (spec §8.1). `sig` already has the type's own parameters filled in
    /// from the receiver, so only the method's remain.
    fn solve_method(
        &mut self,
        sig: FnSig,
        seg: Option<&PathSegment>,
        args: &[Expr],
        span: Span,
        name: &str,
    ) -> (FnSig, Vec<Ty>) {
        if !sig.is_generic() {
            if let Some(seg) = seg.filter(|s| !s.generics.is_empty()) {
                self.cx.error_help(
                    codes::WRONG_TYPE_ARG_COUNT,
                    seg.span,
                    format!("`{name}` takes no type arguments"),
                    "not a generic function",
                    format!("`{name}` has no type parameters of its own; remove `<…>` (spec §8.1)"),
                );
            }
            return (sig, Vec::new());
        }
        let bare;
        let seg = match seg {
            Some(s) => s,
            None => {
                bare = PathSegment {
                    span,
                    name: Ident { name: name.to_owned(), span },
                    generics: Vec::new(),
                };
                &bare
            }
        };
        self.solve_call(&sig, seg, args, span, name)
    }

    /// Does the type chosen for a parameter satisfy the parameter's bounds (spec §8.1)?
    ///
    /// The bounds are written in terms of the parameter, so `T: Mul` is `T: Mul<T>`; when
    /// `T` is solved to `Vec3` the obligation becomes `Mul<Vec3>`, which is why the bound is
    /// substituted before it is checked.
    fn check_bounds(&mut self, def: &ParamDef, chosen: &Ty, span: Span) {
        if chosen.is_wildcard() {
            return;
        }
        let mut map = HashMap::new();
        map.insert(def.param.id, chosen.clone());
        for bound in &def.bounds {
            let wanted = TraitRef {
                trait_sym: bound.trait_sym,
                args: bound.args.iter().map(|a| ty::substitute(a, &map)).collect(),
                span: bound.span,
            };
            if self.satisfies(chosen, &wanted) {
                continue;
            }
            let t = self.cx.name_of(chosen);
            let shown = self.trait_ref_name(&wanted);
            let p = def.name();
            // A type the language provides has no module for an `impl` to live in, so the
            // way to give it a trait is a struct around it (spec §7.6).
            let help = if chosen.named().is_some() {
                format!("implement it: `impl {shown} for {t} {{ … }}` (spec §7.6)")
            } else {
                format!(
                    "`{t}` is built in, and implements only what the language gives it; wrap \
                     it in a struct that implements `{shown}` (spec §7.6)"
                )
            };
            self.cx.error_help(
                codes::UNSATISFIED_BOUND,
                span,
                format!("`{t}` does not implement `{shown}`"),
                format!("`{p}` is bounded by `{shown}`"),
                help,
            );
        }
    }

    /// `Mul<f32>` — a trait reference as it would be written, for a diagnostic.
    fn trait_ref_name(&self, r: &TraitRef) -> String {
        let name = self.cx.res.symbol(r.trait_sym).name.clone();
        // A default argument is not shown: `Mul<Vec3>` on a `Vec3` is spelled `Mul`.
        let params = self.cx.table.type_params(r.trait_sym);
        let interesting: Vec<&Ty> = r
            .args
            .iter()
            .zip(params)
            .filter(|(_, p)| p.default.is_none())
            .map(|(a, _)| a)
            .collect();
        if interesting.is_empty() {
            return name;
        }
        let args: Vec<String> = interesting.iter().map(|a| self.cx.name_of(a)).collect();
        format!("{name}<{}>", args.join(", "))
    }

    /// Does `t` satisfy `wanted`, directly or through a supertrait chain (spec §7.6)?
    ///
    /// A `Ty::Param` satisfies a bound its own bounds already provide, which is what lets a
    /// generic body call another generic function: `fn a<T: Ord>` may call `fn b<U: Ord>`
    /// with its own `T`.
    fn satisfies(&self, t: &Ty, wanted: &TraitRef) -> bool {
        self.cx.table.implements(t, wanted)
    }

    /// What `t` is known to implement: a parameter's bounds, or the traits the `impl`s that
    /// apply to it implement (spec §7.6).
    fn declared_traits_of(&self, t: &Ty) -> Vec<TraitRef> {
        match t {
            Ty::Param(p) => self.cx.table.bounds_of(p).to_vec(),
            _ => self.cx.table.traits_of(t),
        }
    }

    /// What `for x in it` binds `x` to when `it` implements the prelude's `Iterate`: the
    /// implementer's chosen `Item`, read off `get` (spec §8.5, §10.4). `None` when the type
    /// does not implement it at all.
    fn iterate_item(&mut self, t: &Ty, span: Span) -> Option<Ty> {
        let t = self.subst(t);
        let iterate = self.cx.prelude_trait("Iterate")?;
        let r = TraitRef { trait_sym: iterate, args: Vec::new(), span };
        if !self.satisfies(&t, &r) {
            return None;
        }
        if let Ty::Param(p) = &t {
            // `T: Iterate` gives the loop variable the type `T::Item`, and monomorphization
            // cannot yet trace that back to the implementer that chose it — so this is
            // refused here rather than emitted as broken C++.
            let name = p.name.clone();
            self.cx.error_help(
                codes::CODEGEN_UNSUPPORTED,
                span,
                format!("`for` over a generic `{name}: Iterate` is not supported yet"),
                "its element type would be the projection `T::Item`",
                "iterate a concrete type, or pass a slice — see the deferral register in \
                 docs/PROGRESS.md",
            );
            return Some(Ty::Error);
        }
        let sig = self.cx.table.trait_method(&r, &t, "get")?;
        Some(sig.ret)
    }

    /// `v.name(…)` reaching a trait's method through an implementation for `v`'s type (spec
    /// §7.6) — one the `impl` writes, or a default the trait supplies. `None` when no
    /// implementation provides `name`.
    ///
    /// A type's own methods have already been tried, so they win over a trait's. Two
    /// implementations of one trait at different arguments are chosen between by the first
    /// argument (§18.27); two *traits* both providing `name` is ambiguous, and the answer is
    /// to name the trait: `Describe::name(&v)`.
    #[allow(clippy::too_many_arguments)]
    fn trait_method_call(
        &mut self,
        recv: &Checked,
        base: &Expr,
        peeled: &Ty,
        name: &Ident,
        args: &[Expr],
        e: &Expr,
    ) -> Option<Checked> {
        let span = e.span;
        if self.is_special_trait_method(peeled, name) {
            return Some(self.special_member_called(name, args, e));
        }
        let mut found: Vec<(SymbolId, FnSig)> = Vec::new();
        for (idx, r, _) in self.cx.table.impls_for(peeled) {
            let written = self.cx.table.impls()[idx].methods.contains_key(&name.name);
            let default = self
                .cx
                .table
                .struct_sig(r.trait_sym)
                .and_then(|s| s.methods.get(&name.name))
                .is_some_and(|m| m.has_body);
            if !written && !default {
                continue;
            }
            if let Some(sig) = self.cx.table.trait_method(&r, peeled, &name.name) {
                found.push((r.trait_sym, sig));
            }
        }
        if found.is_empty() {
            return None;
        }
        let (trait_sym, sig) = if found.len() == 1 {
            found.remove(0)
        } else if found.iter().all(|(t, _)| *t == found[0].0) {
            let sigs: Vec<FnSig> = found.iter().map(|(_, s)| s.clone()).collect();
            let arg = args.first().map(|a| self.expr_quiet(a));
            let i = self.pick_overload(&sigs, arg.as_ref(), span, &name.name);
            found.swap_remove(i)
        } else {
            let traits: Vec<String> =
                found.iter().map(|(t, _)| self.cx.res.symbol(*t).name.clone()).collect();
            let first = traits[0].clone();
            self.cx.error_help(
                codes::AMBIGUOUS_CALL,
                name.span,
                format!("`{}` is provided by both `{}`", name.name, traits.join("` and `")),
                "ambiguous method",
                format!(
                    "name the trait to choose one: `{first}::{}(&v, …)` (spec §7.6)",
                    name.name
                ),
            );
            found.remove(0)
        };
        if sig.is_mut {
            self.require_mutable_receiver(base, &recv.ty, &name.name);
        }
        if sig.is_unsafe {
            self.require_unsafe(span, &format!("calling `unsafe fn {}`", name.name));
        }
        let args = self.check_args(&sig, args, span, &name.name);
        let ty = fallible_result(&sig, self.subst(&sig.ret));
        let callee = hir::Callee::Method {
            recv: recv.id,
            owner: trait_sym,
            name: name.name.clone(),
            method_args: Vec::new(),
        };
        Some(self.mk(e, ty, hir::ExprKind::Call { callee, args }))
    }

    /// Is `name` `drop` or `copy` on a type whose `impl Drop` / `impl Copy` provides it?
    /// Those are the destructor and the copy constructor, and nothing calls them by name.
    fn is_special_trait_method(&self, ty: &Ty, name: &Ident) -> bool {
        let trait_named = match name.name.as_str() {
            "drop" => "Drop",
            "copy" => "Copy",
            _ => return false,
        };
        let Some(special) = self.cx.prelude_trait(trait_named) else { return false };
        self.cx.table.traits_of(ty).iter().any(|r| r.trait_sym == special)
    }

    /// `x.drop()` or `x.copy(y)`: both run by themselves — at the end of the scope, and on
    /// every copy (spec §6.4, §6.5) — so calling one would run it twice.
    fn special_member_called(&mut self, name: &Ident, args: &[Expr], e: &Expr) -> Checked {
        let help = if name.name == "drop" {
            "a value's `drop` runs by itself at the end of its scope; to end its life early, \
             `move` it into a scope that ends (spec §6.5)"
        } else {
            "`copy` runs by itself on every copy: write `let b = a` (spec §6.3)"
        };
        self.cx.error_help(
            codes::NO_SUCH_METHOD,
            name.span,
            format!("`{}` cannot be called by name", name.name),
            "runs by itself",
            help,
        );
        for a in args {
            self.expr(a, None);
        }
        self.mk(e, Ty::Error, hir::ExprKind::Error)
    }

    /// Is `name` on `sym` a function with no receiver? Overloads of one name agree on it:
    /// a receiver-less overload beside a method would make `T::name(x)` mean two things.
    fn is_associated_fn(&self, sym: SymbolId, name: &str) -> bool {
        let Some(s) = self.cx.table.struct_sig(sym) else { return false };
        let Some(keys) = s.overloads.get(name) else { return false };
        keys.iter().filter_map(|k| s.methods.get(k)).any(|m| m.takes_self.is_none())
    }

    /// `Quat::identity()`, `Buffer<f32>::with_capacity(8)` (spec §7.3).
    fn associated_call(
        &mut self,
        owner: SymbolId,
        prefix: &Path,
        seg: &PathSegment,
        args: &[Expr],
        e: &Expr,
    ) -> Checked {
        let span = e.span;
        let name = &seg.name;
        // Written on the path, or — inside the type's own body — its parameters in scope.
        let generic = !self.cx.table.type_params(owner).is_empty();
        let type_args = match prefix.segments.last() {
            Some(seg) if generic => self.cx.type_args(owner, seg),
            _ => Vec::new(),
        };
        let Some(sig) = self.method_by_args(owner, &type_args, &name.name, args, span) else {
            for a in args {
                self.expr(a, None);
            }
            return self.mk(e, Ty::Error, hir::ExprKind::Error);
        };
        self.check_member_visible(owner, &name.name, sig.vis, sig.span, name.span, "function");
        if sig.is_unsafe {
            self.require_unsafe(span, &format!("calling `unsafe fn {}`", name.name));
        }
        // Named as it is written, so "write it out" says `Crate::fresh<T>(…)`.
        let shown = format!("{}::{}", self.cx.res.symbol(owner).name, name.name);
        let (sig, method_args) = self.solve_method(sig, Some(seg), args, span, &shown);
        let checked_args = self.check_args(&sig, args, span, &name.name);
        let ty = fallible_result(&sig, self.subst(&sig.ret));
        let callee = hir::Callee::Assoc { owner, type_args, name: name.name.clone(), method_args };
        self.mk(e, ty, hir::ExprKind::Call { callee, args: checked_args })
    }

    /// `UiElement::draw(self, …)` — a base's version of a method (spec §7.3, 18.32).
    ///
    /// The receiver is the first argument, as it is for a trait's qualified call. The
    /// point is the *static* dispatch: inside `override fn draw`, `UiElement::draw(self)`
    /// runs the base's body, where `self.draw()` would call this one again forever.
    fn qualified_base_call(
        &mut self,
        base: SymbolId,
        method: &Ident,
        args: &[Expr],
        e: &Expr,
    ) -> Checked {
        let span = e.span;
        let bname = self.cx.res.symbol(base).name.clone();
        let Some((first, rest)) = args.split_first() else {
            self.cx.error_help(
                codes::WRONG_ARG_COUNT,
                span,
                format!("`{bname}::{}` needs the value to call it on", method.name),
                "no receiver",
                format!("write `{bname}::{}(self, …)` (spec §7.3)", method.name),
            );
            return self.mk(e, Ty::Error, hir::ExprKind::Error);
        };

        let recv = self.expr(first, None);
        let peeled = recv.ty.peeled().clone();
        // The receiver must actually *be* one of these: the base itself, or something
        // that inherits from it. Otherwise this names a method of an unrelated type.
        let ok = match peeled.named() {
            Some(s) => s == base || self.cx.table.inherits_from(s, base),
            None => false,
        };
        if !ok {
            let shown = self.cx.name_of(&recv.ty);
            self.cx.error_help(
                codes::TYPE_MISMATCH,
                first.span,
                format!("`{shown}` does not inherit from `{bname}`"),
                format!("not a `{bname}`"),
                format!(
                    "`{bname}::{}` is called on a `{bname}` or on something that inherits \
                     from one (spec §7.3)",
                    method.name
                ),
            );
            for a in rest {
                self.expr(a, None);
            }
            return self.mk(e, Ty::Error, hir::ExprKind::Error);
        }

        let Some(sig) = self.method_by_args(base, &[], &method.name, rest, span) else {
            self.cx.error(
                codes::NO_SUCH_METHOD,
                method.span,
                format!("`{bname}` has no method `{}`", method.name),
                "unknown method",
            );
            for a in rest {
                self.expr(a, None);
            }
            return self.mk(e, Ty::Error, hir::ExprKind::Error);
        };
        self.check_member_visible(base, &method.name, sig.vis, sig.span, method.span, "method");
        if sig.is_mut {
            self.require_mutable_receiver(first, &recv.ty, &method.name);
        }
        if sig.is_unsafe {
            self.require_unsafe(span, &format!("calling `unsafe fn {}`", method.name));
        }
        let args = self.check_args(&sig, rest, span, &method.name);
        let ty = fallible_result(&sig, self.subst(&sig.ret));
        let callee =
            hir::Callee::BaseMethod { recv: recv.id, owner: base, name: method.name.clone() };
        self.mk(e, ty, hir::ExprKind::Call { callee, args })
    }

    /// `Describe::name(&v, …)` — a trait's method named through the trait, the way to choose
    /// between two traits that both provide `name` (spec §7.6). The first argument is the
    /// receiver, borrowed as a method call would borrow it.
    fn qualified_trait_call(
        &mut self,
        tsym: SymbolId,
        method: &Ident,
        args: &[Expr],
        e: &Expr,
    ) -> Checked {
        let span = e.span;
        let tname = self.cx.res.symbol(tsym).name.clone();
        if Some(tsym) == self.cx.prelude_trait("Drop")
            || Some(tsym) == self.cx.prelude_trait("Copy")
        {
            return self.special_member_called(method, args, e);
        }
        let Some((first, rest)) = args.split_first() else {
            self.cx.error_help(
                codes::WRONG_ARG_COUNT,
                span,
                format!("`{tname}::{}` needs the value to call it on", method.name),
                "no receiver",
                format!("pass it first: `{tname}::{}(&v)` (spec §7.6)", method.name),
            );
            return self.mk(e, Ty::Error, hir::ExprKind::Error);
        };
        let recv_expr = match &first.kind {
            ExprKind::Ref { expr, .. } => expr.as_ref(),
            _ => first,
        };
        let recv = self.expr(recv_expr, None);
        let peeled = recv.ty.peeled().clone();
        let refs: Vec<TraitRef> = match &peeled {
            Ty::Param(p) => {
                let bounds = self.cx.table.bounds_of(p).to_vec();
                self.cx.table.trait_closure(&bounds, &peeled)
            }
            _ => self.cx.table.traits_of(&peeled),
        };
        let found: Vec<FnSig> = refs
            .iter()
            .filter(|r| r.trait_sym == tsym)
            .filter_map(|r| self.cx.table.trait_method(r, &peeled, &method.name))
            .collect();
        if found.is_empty() {
            if !peeled.is_wildcard() {
                let t = self.cx.name_of(&peeled);
                self.cx.error_help(
                    codes::UNSATISFIED_BOUND,
                    first.span,
                    format!("`{t}` does not implement `{tname}::{}`", method.name),
                    format!("no `impl {tname}` for this"),
                    format!("`{tname}::{}` can only be called on a type that implements `{tname}` (spec §7.6)", method.name),
                );
            }
            for a in rest {
                self.expr(a, None);
            }
            return self.mk(e, Ty::Error, hir::ExprKind::Error);
        }
        let i = if found.len() == 1 {
            0
        } else {
            let arg = rest.first().map(|a| self.expr_quiet(a));
            self.pick_overload(&found, arg.as_ref(), span, &method.name)
        };
        let sig = found[i].clone();
        if sig.is_mut {
            self.require_mutable_receiver(recv_expr, &recv.ty, &method.name);
        }
        if sig.is_unsafe {
            self.require_unsafe(span, &format!("calling `unsafe fn {}`", method.name));
        }
        let args = self.check_args(&sig, rest, span, &method.name);
        let ty = fallible_result(&sig, self.subst(&sig.ret));
        let callee = hir::Callee::Method {
            recv: recv.id,
            owner: tsym,
            name: method.name.clone(),
            method_args: Vec::new(),
        };
        self.mk(e, ty, hir::ExprKind::Call { callee, args })
    }

    /// `.x` or `.x()` on a generic parameter that its bounds do not provide (spec §8.1).
    ///
    /// The message names the bounds, because that is the actionable part: the fix is
    /// either to use something the bound already has, or to add a bound. This is reported
    /// at the line inside the generic body, never at an instantiation (§8.6).
    fn no_such_member_on_param(&mut self, p: &ty::Param, name: &Ident, what: &str) {
        let bounds = self.cx.table.bounds_of(p).to_vec();
        let t = &p.name;
        let member = &name.name;
        let (label, help) = if bounds.is_empty() {
            (
                format!("`{t}` has no bounds, so it has no {what}s"),
                format!(
                    "an unbounded `{t}` supports only being moved, default-initialised, \
                     dropped, and queried with `{t}::` — add a bound like `{t}: SomeTrait` \
                     to give it more (spec §8.2)"
                ),
            )
        } else {
            let names: Vec<String> = bounds.iter().map(|b| self.trait_ref_name(b)).collect();
            let listed = names.join(" + ");
            (
                format!("not provided by `{listed}`"),
                format!(
                    "`{t}` is bounded by `{listed}`, and that is the whole of what it can \
                     do here — add `{member}` to one of those traits, or add a bound to \
                     `{t}` (spec §8.1)"
                ),
            )
        };
        self.cx.error_help(
            codes::UNSATISFIED_BOUND,
            name.span,
            format!("`{t}` has no {what} `{member}`"),
            label,
            help,
        );
    }

    /// A method one of a generic parameter's bounds provides, already read through that
    /// bound (spec §8.1).
    ///
    /// Supertraits count: a `T: Ord` may use `Eq`'s methods, because `trait Ord : Eq` means
    /// every `Ord` is an `Eq` (spec §7.6). The returned signature has both the trait's own
    /// parameters and `Self` substituted, so it is stated in terms the caller can check
    /// arguments against directly.
    fn param_method(&mut self, p: &ty::Param, name: &str) -> Option<(SymbolId, FnSig)> {
        let self_ty = Ty::Param(p.clone());
        let bounds = self.cx.table.bounds_of(p).to_vec();
        for r in self.cx.table.trait_closure(&bounds, &self_ty) {
            if let Some(sig) = self.cx.table.trait_method(&r, &self_ty, name) {
                return Some((r.trait_sym, sig));
            }
        }
        None
    }

    fn check_member_visible(
        &mut self,
        owner: SymbolId,
        member: &str,
        vis: Vis,
        decl_span: Span,
        use_span: Span,
        what: &str,
    ) {
        let symbol = self.cx.res.symbol(owner);
        let owner_module = symbol.module;
        let owner_name = symbol.name.clone();
        let decl_file = symbol.file;

        // Inside the struct's own methods everything is reachable (spec §4). A trait's
        // methods are as visible as the trait: they are its whole interface, and seeing the
        // trait is already the visibility check (spec §4, §7.6).
        if self.self_ty == Some(owner) || symbol.kind == SymbolKind::Trait {
            return;
        }
        let ok = match vis {
            Vis::Public => true,
            // `protected` is reachable from the type that declares it and from the types
            // that inherit from it — wherever those are written, since inheriting is what
            // grants the access, not sharing a module (spec §4, §7.3, 18.32).
            Vis::Protected => self.self_ty.is_some_and(|s| self.cx.table.inherits_from(s, owner)),
            Vis::Module | Vis::Private => owner_module == self.cx.module && vis == Vis::Module,
        };
        if ok {
            return;
        }
        let module_name = self.cx.ws.module(owner_module).name.clone();
        let help = match vis {
            Vis::Module => format!("`pub(module)` is visible inside module `{module_name}` only; mark it `pub` to export it (spec §4)"),
            Vis::Protected => format!(
                "`protected` is visible to `{owner_name}` and the types that inherit from \
                 it, and to nothing else — inherit from `{owner_name}`, or mark this \
                 {what} `pub` (spec §4, §7.3)"
            ),
            _ => format!("this {what} is private to `{owner_name}`; mark it `pub` or `pub(module)` (spec §4)"),
        };
        self.cx.diags.push(
            dz_diag::Diagnostic::error(
                codes::PRIVATE_FIELD,
                format!("{what} `{owner_name}::{member}` is not visible here"),
            )
            .at(self.cx.file, use_span, format!("private {what}"))
            .with_label(dz_diag::Label::secondary(decl_file, decl_span, "declared here"))
            .with_help(help),
        );
    }

    fn call(&mut self, callee: &Expr, args: &[Expr], e: &Expr) -> Checked {
        let span = e.span;
        // A method call parses as a call on a field access.
        if let ExprKind::Field { base, name, generics } = &callee.kind {
            let mark = self.hir.mark();
            let mut recv = self.expr(base, None);
            // `5.hash()`, `2.5.sqrt()`: a literal receiver has nothing else to give it a type,
            // so it takes the one `let` would — `i32` or `f64` (spec §2.5) — and is checked
            // again with it, leaving no untyped node behind for codegen.
            if matches!(recv.ty, Ty::IntLit | Ty::FloatLit)
                && matches!(base.kind, ExprKind::Int(_) | ExprKind::Float(_))
            {
                let wanted = recv.ty.defaulted();
                self.hir.rollback(mark);
                recv = self.expr(base, Some(&wanted));
            }
            let base_ty = recv.ty.clone();
            let peeled = base_ty.peeled().clone();
            if matches!(base_ty.peeled_refs(), Ty::Ptr { .. } | Ty::OwnPtr { .. }) {
                self.require_unsafe(callee.span, "calling a method through a `^T`");
            }

            // A method on a generic parameter comes from its bounds, and from nothing else
            // (spec §8.1). Resolution is static: the bound names one trait method, and
            // monomorphization turns the call into a direct one (§7.6).
            if let Ty::Param(p) = &peeled {
                self.no_method_type_args(generics, name);
                let found = self.param_method(p, &name.name);
                let Some((trait_sym, sig)) = found else {
                    self.no_such_member_on_param(p, name, "method");
                    for a in args {
                        self.expr(a, None);
                    }
                    return self.mk(e, Ty::Error, hir::ExprKind::Error);
                };
                if sig.is_mut {
                    self.require_mutable_receiver(base, &base_ty, &name.name);
                }
                // `Self` in the trait's signature is this parameter.
                let sig = instantiate_self(&sig, &peeled);
                let args = self.check_args(&sig, args, span, &name.name);
                let ty = fallible_result(&sig, self.subst(&sig.ret));
                let callee = hir::Callee::Method {
                    recv: recv.id,
                    owner: trait_sym,
                    name: name.name.clone(),
                    method_args: Vec::new(),
                };
                return self.mk(e, ty, hir::ExprKind::Call { callee, args });
            }

            // `n.hash()`, `a.cmp(&b)` on a primitive: the prelude traits the machine's own
            // types implement without an `impl` (spec §11.2, §14.2). Codegen lowers the call
            // to the operator or the shim function it stands for.
            let builtin_receiver =
                matches!(peeled, Ty::Int(_) | Ty::Float(_) | Ty::Bool | Ty::Char | Ty::Str)
                    || self.cx.table.is_enum(&peeled);
            if builtin_receiver {
                for trait_name in ["Hash", "Eq", "Ord", "Float", "Integer", "Number"] {
                    let Some(trait_sym) = self.cx.table.prelude_trait_named(trait_name) else {
                        continue;
                    };
                    if !self.cx.table.builtin_implements(&peeled, trait_name, &[]) {
                        continue;
                    }
                    let Some(sig) = self
                        .cx
                        .table
                        .struct_sig(trait_sym)
                        .and_then(|s| s.methods.get(&name.name))
                        .cloned()
                    else {
                        continue;
                    };
                    self.no_method_type_args(generics, name);
                    let sig = instantiate_self(&sig, &peeled);
                    let args = self.check_args(&sig, args, span, &name.name);
                    let ty = fallible_result(&sig, self.subst(&sig.ret));
                    let callee = hir::Callee::Method {
                        recv: recv.id,
                        owner: trait_sym,
                        name: name.name.clone(),
                        method_args: Vec::new(),
                    };
                    return self.mk(e, ty, hir::ExprKind::Call { callee, args });
                }
                // `n.sqrt()` on an integer: the method exists, on the other kind of number.
                let other = match peeled {
                    Ty::Int(_) => Some(("Float", "`f32` and `f64`", "as f32")),
                    Ty::Float(_) => Some(("Integer", "the integer types", "as i32")),
                    _ => None,
                };
                let declared_there = other.and_then(|(trait_name, owners, cast)| {
                    let t = self.cx.table.prelude_trait_named(trait_name)?;
                    let sig = self.cx.table.struct_sig(t)?;
                    sig.methods.contains_key(&name.name).then_some((owners, cast))
                });
                if let Some((owners, cast)) = declared_there {
                    let shown = self.cx.name_of(&peeled);
                    self.cx.error_help(
                        codes::NO_SUCH_METHOD,
                        name.span,
                        format!("`{shown}` has no method `{}`", name.name),
                        format!("`{}` is a method of {owners}", name.name),
                        format!(
                            "convert first — `(value {cast}).{}(…)` — since numbers never \
                             convert on their own (spec §5.1, §14.4)",
                            name.name
                        ),
                    );
                    for a in args {
                        self.expr(a, None);
                    }
                    return self.mk(e, Ty::Error, hir::ExprKind::Error);
                }
            }

            if let Some(sym) = peeled.named() {
                {
                    let type_args: Vec<Ty> = match &peeled {
                        Ty::Named { args, .. } => args.clone(),
                        _ => Vec::new(),
                    };
                    // A base's methods are the derived type's, so the lookup walks the
                    // chain; the type that declared it is the one whose visibility applies
                    // and the one the HIR records (spec §7.3, 18.32).
                    let owner = self.cx.table.method_owner(sym, &name.name).unwrap_or(sym);
                    if let Some(sig) =
                        self.method_by_args(owner, &type_args, &name.name, args, span)
                    {
                        self.check_member_visible(
                            owner, &name.name, sig.vis, sig.span, name.span, "method",
                        );
                        // No receiver: it is the type's function, not the value's (§7.3).
                        if sig.takes_self.is_none() {
                            let tname = self.cx.res.symbol(owner).name.clone();
                            self.cx.error_help(
                                codes::NO_SUCH_METHOD,
                                name.span,
                                format!("`{tname}::{}` takes no `self`", name.name),
                                "called on a value",
                                format!(
                                    "call it through the type: `{tname}::{}(…)` (spec §7.3)",
                                    name.name
                                ),
                            );
                            for a in args {
                                self.expr(a, None);
                            }
                            return self.mk(e, Ty::Error, hir::ExprKind::Error);
                        }
                        if sig.is_mut {
                            self.require_mutable_receiver(base, &base_ty, &name.name);
                        }
                        if sig.is_unsafe {
                            self.require_unsafe(
                                span,
                                &format!("calling `unsafe fn {}`", name.name),
                            );
                        }
                        let written = PathSegment {
                            span: name.span,
                            name: name.clone(),
                            generics: generics.clone(),
                        };
                        let (sig, method_args) =
                            self.solve_method(sig, Some(&written), args, span, &name.name);
                        let args = self.check_args(&sig, args, span, &name.name);
                        let ty = fallible_result(&sig, self.subst(&sig.ret));
                        let callee = hir::Callee::Method {
                            recv: recv.id,
                            owner,
                            name: name.name.clone(),
                            method_args,
                        };
                        return self.mk(e, ty, hir::ExprKind::Call { callee, args });
                    }
                    // A trait's method, through an implementation for this type (spec §7.6).
                    if let Some(c) = self.trait_method_call(&recv, base, &peeled, name, args, e) {
                        self.no_method_type_args(generics, name);
                        return c;
                    }
                    // Not a method: fall through so the field-access path reports.
                    let sig_has_field = self
                        .cx
                        .table
                        .struct_sig(sym)
                        .map(|s| s.fields.iter().any(|f| f.name == name.name))
                        .unwrap_or(false);
                    // A generic implementation would provide it, but the arguments miss its
                    // bound: `List<Plain>.sort()` wants `Plain: Ord` (spec §7.6, §8.1).
                    if !sig_has_field {
                        if let Some((tr, arg, bound)) =
                            self.cx.table.unmet_bound_for(&peeled, &name.name)
                        {
                            let t = self.cx.name_of(&peeled);
                            let a = self.cx.name_of(&arg);
                            let b = self.trait_ref_name(&bound);
                            let tname = self.cx.res.symbol(tr).name.clone();
                            self.cx.error_help(
                                codes::UNSATISFIED_BOUND,
                                name.span,
                                format!(
                                    "`{t}` has `{}` only when `{a}` implements `{b}`",
                                    name.name
                                ),
                                format!("`{a}` does not implement `{b}`"),
                                format!(
                                    "`{}` comes from `{tname}`, implemented for `{t}` when its \
                                     argument is `{b}`: implement `{b}` for `{a}` (spec §7.6)",
                                    name.name
                                ),
                            );
                            for a in args {
                                self.expr(a, None);
                            }
                            return self.mk(e, Ty::Error, hir::ExprKind::Error);
                        }
                    }
                    if !sig_has_field {
                        let owner = self.cx.res.symbol(sym).name.clone();
                        // A base's methods are among the ones this type has, so they are
                        // among the ones worth suggesting (spec §7.3).
                        let names: Vec<String> = self
                            .cx
                            .table
                            .with_bases(sym)
                            .iter()
                            .filter_map(|s| self.cx.table.struct_sig(*s))
                            .flat_map(|s| s.overloads.keys().cloned())
                            .collect();
                        let suggestion =
                            dz_resolve::closest(&name.name, names.iter().map(String::as_str));
                        let mut d = dz_diag::Diagnostic::error(
                            codes::NO_SUCH_METHOD,
                            format!("`{owner}` has no method `{}`", name.name),
                        )
                        .at(self.cx.file, name.span, "unknown method");
                        if let Some(s) = suggestion {
                            d = d.with_help(format!("did you mean `{s}`?"));
                        }
                        self.cx.diags.push(d);
                        for a in args {
                            self.expr(a, None);
                        }
                        return self.mk(e, Ty::Error, hir::ExprKind::Error);
                    }
                }
            } else if let Some(c) = self.trait_method_call(&recv, base, &peeled, name, args, e) {
                // A type with no methods of its own — `f32`, say — has a trait's, through an
                // implementation for it (spec §7.6).
                self.no_method_type_args(generics, name);
                return c;
            }
        }

        // `Describe::name(&v)` — a trait's method named through the trait (spec §7.6).
        if let ExprKind::Path(p) = &callee.kind {
            if p.segments.len() >= 2 {
                let prefix =
                    Path { span: p.span, segments: p.segments[..p.segments.len() - 1].to_vec() };
                if let Some(tsym) = self.path_symbol(&prefix) {
                    if self.cx.res.symbol(tsym).kind == SymbolKind::Trait {
                        let method = &p.segments[p.segments.len() - 1].name;
                        return self.qualified_trait_call(tsym, method, args, e);
                    }
                    // `UiElement::draw(self)` — a base's version, called without virtual
                    // dispatch, which is how an `override` builds on what it replaces
                    // (spec §7.3, 18.32).
                    if self.cx.res.symbol(tsym).kind == SymbolKind::Struct
                        && self
                            .cx
                            .table
                            .struct_sig(tsym)
                            .is_some_and(|s| !s.ctors.is_empty() || !s.method_keys.is_empty())
                    {
                        let method = p.segments[p.segments.len() - 1].name.clone();
                        if self
                            .cx
                            .table
                            .struct_sig(tsym)
                            .is_some_and(|s| s.overloads.contains_key(&method.name))
                        {
                            // A function with no receiver is the type's own, called with
                            // exactly its arguments (spec §7.3).
                            if self.is_associated_fn(tsym, &method.name) {
                                let seg = &p.segments[p.segments.len() - 1];
                                return self.associated_call(tsym, &prefix, seg, args, e);
                            }
                            return self.qualified_base_call(tsym, &method, args, e);
                        }
                    }
                }
            }
        }

        // A constructor call: `Buffer(8)`, or `Buffer<f32>(8)` for a generic struct.
        if let ExprKind::Path(p) = &callee.kind {
            if let Some(sym) = self.path_symbol(p) {
                if self.cx.res.symbol(sym).kind == SymbolKind::Struct {
                    let ctors =
                        self.cx.table.struct_sig(sym).map(|s| s.ctors.clone()).unwrap_or_default();
                    if let Some(sig) = ctors.first().cloned() {
                        let name = self.cx.res.symbol(sym).name.clone();
                        // A generic struct's parameters are solved exactly as a generic
                        // function's are: written (`Buffer<f32>(8)`) or inferred from the
                        // arguments (spec §8.1). The result is the instantiation.
                        let (sig, ty) = if self.cx.table.is_generic(sym) {
                            let generic =
                                FnSig { generics: self.cx.table.type_params(sym).to_vec(), ..sig };
                            let last = p.segments.last().expect("a path has segments");
                            let (solved, targs) =
                                self.solve_call(&generic, last, args, span, &name);
                            (solved, Ty::Named { sym, args: targs })
                        } else {
                            (sig, Ty::plain(sym))
                        };
                        let args = self.check_args(&sig, args, span, &name);
                        self.refuse_abstract(sym, span);
                        let callee = hir::Callee::Ctor { owner: sym, index: 0 };
                        return self.mk(e, ty, hir::ExprKind::Call { callee, args });
                    }
                }
            }
            // A named function: use its signature, which knows about default arguments
            // (spec §7.2). The generic `Ty::Fn` path below cannot, since a function type
            // does not record which parameters have defaults.
            if let Some(sym) = self.path_symbol(p) {
                if let Some(ItemSig::Fn(sig)) = self.cx.table.get(sym) {
                    let mut sig = sig.clone();
                    let name = self.cx.res.symbol(sym).name.clone();
                    // A generic function is called with concrete types, so its parameters
                    // are solved from the arguments before the call is checked (spec §8.1).
                    // The solution is kept on the call: it is what monomorphization
                    // instantiates the function with.
                    let mut type_args = Vec::new();
                    if sig.is_generic() {
                        let last = p.segments.last().expect("a path has segments");
                        let (solved, targs) = self.solve_call(&sig, last, args, span, &name);
                        sig = solved;
                        type_args = targs;
                    }
                    if sig.is_unsafe {
                        self.require_unsafe(span, &format!("calling `unsafe fn {name}`"));
                    }
                    let args = self.check_args(&sig, args, span, &name);
                    let ty = fallible_result(&sig, self.subst(&sig.ret));
                    let callee = hir::Callee::Fn { sym, type_args };
                    return self.mk(e, ty, hir::ExprKind::Call { callee, args });
                }
            }

            // Builtins are untyped until the stdlib exists: arity is checked by
            // `builtin_arity`, and a formatting call's literal by `check_format` (§12.9).
            // `str::from_utf8(bytes)`: the checked way from bytes to text — `none` when they
            // are not valid UTF-8 (spec §5.3). `bytes as str` is the unchecked one.
            if p.segments.len() == 2
                && p.segments[0].name.name == "str"
                && p.segments[1].name.name == "from_utf8"
            {
                let want = Ty::Slice { mutable: false, elem: Rc::new(Ty::Int(IntTy::U8)) };
                if args.len() != 1 {
                    self.cx.error(
                        codes::WRONG_ARG_COUNT,
                        span,
                        format!("`str::from_utf8` takes 1 argument, found {}", args.len()),
                        "wrong number of arguments",
                    );
                    for a in args {
                        self.expr(a, None);
                    }
                    return self.mk(e, Ty::Error, hir::ExprKind::Error);
                }
                let c = self.expr(&args[0], Some(&want));
                self.expect(&want, &c.ty, args[0].span);
                let kind =
                    hir::ExprKind::Builtin { name: "str_from_utf8".to_owned(), args: vec![c.id] };
                return self.mk(e, Ty::Optional(Rc::new(Ty::Str)), kind);
            }
            if p.segments.len() == 1
                && dz_resolve::BUILTIN_FNS.contains(&p.segments[0].name.name.as_str())
            {
                let name = p.segments[0].name.name.clone();
                if matches!(name.as_str(), "typeinfo_of" | "typeid_of" | "typeinfo") {
                    return self.reflect_call(e, &name, &p.segments[0], args, span);
                }
                // `sizeof(T)` and `alignof(T)` take a *type*, and are spellings of `T::size`
                // and `T::align` (spec §5.5).
                if matches!(name.as_str(), "sizeof" | "alignof") && args.len() == 1 {
                    if let Some(q) = self.type_argument_query(&name, &args[0]) {
                        return self.mk(e, q.0, q.1);
                    }
                }
                // Codegen reads these by position, so a missing argument must be a Dazzle
                // diagnostic rather than a panic in the compiler or bad C++ (invariant 7).
                if let Some((lo, hi)) = builtin_arity(&name) {
                    if args.len() < lo || args.len() > hi {
                        let want = if lo == hi { lo.to_string() } else { format!("{lo} or {hi}") };
                        self.cx.error(
                            codes::WRONG_ARG_COUNT,
                            span,
                            format!(
                                "`{name}` takes {want} {}, found {}",
                                plural(hi, "argument"),
                                args.len()
                            ),
                            "wrong number of arguments",
                        );
                        for a in args {
                            self.expr(a, None);
                        }
                        return self.mk(e, Ty::Error, hir::ExprKind::Error);
                    }
                }
                // The placement primitives and the heap's release write memory nothing
                // tracks (spec §5.2, §14.3). `alloc` only hands a pointer out, which is safe.
                if matches!(name.as_str(), "construct" | "destroy" | "move_out" | "memcpy" | "free")
                {
                    self.require_unsafe(span, &format!("`{name}`"));
                }
                let checked: Vec<Checked> = args.iter().map(|a| self.expr(a, None)).collect();
                // What they write has to be writable: a `^T` promises its pointee is only
                // read (spec §5.2). `free` releases rather than writes, so any pointer does.
                if matches!(name.as_str(), "construct" | "destroy" | "move_out") {
                    if let Some(dst) = args.first() {
                        self.require_writable(dst, &name);
                    }
                }
                if name == "memcpy" {
                    if let (Some(dst), Some(c)) = (args.first(), checked.first()) {
                        if matches!(
                            c.ty,
                            Ty::Ptr { mutable: false, .. } | Ty::OwnPtr { mutable: false, .. }
                        ) {
                            let n = self.cx.name_of(&c.ty);
                            self.cx.error_help(
                                codes::NOT_MUTABLE,
                                dst.span,
                                "`memcpy` writes through its first argument",
                                format!("this is a read-only `{n}`"),
                                "the destination must be a `^mut` pointer (spec §5.2, §14.3)",
                            );
                        }
                    }
                }
                self.check_format(&name, args, &checked, span);
                let ids = checked.iter().map(|c| c.id).collect();
                // `blackbox` hands back exactly what it was given (spec §19 M15). Its type
                // cannot come from the fixed table: the whole point is that the value
                // survives, and a benchmark passes whatever it is measuring.
                // `move_out(p[i])` hands back the value that was in the storage (spec §14.3).
                let ty = if name == "blackbox" || name == "move_out" {
                    checked.first().map(|c| c.ty.clone()).unwrap_or(Ty::Void)
                } else {
                    builtin_call_type(&name)
                };
                return self.mk(e, ty, hir::ExprKind::Builtin { name, args: ids });
            }
        }

        let c = self.expr(callee, None);
        let callee_ty = c.ty.clone();
        let (ty, arg_ids) = match &callee_ty {
            Ty::Fn { params, ret } => {
                let params = params.clone();
                if params.len() != args.len() {
                    let (want, got) = (params.len(), args.len());
                    self.cx.error(
                        codes::WRONG_ARG_COUNT,
                        span,
                        format!("expected {want} {}, found {got}", plural(want, "argument")),
                        "wrong number of arguments",
                    );
                }
                let mut ids = Vec::new();
                for (a, p) in args.iter().zip(&params) {
                    let arg = self.expr(a, Some(p));
                    self.expect(p, &arg.ty, a.span);
                    ids.push(arg.id);
                }
                for a in args.iter().skip(params.len()) {
                    ids.push(self.expr(a, None).id);
                }
                ((**ret).clone(), ids)
            }
            t => {
                if !t.is_wildcard() {
                    let n = self.cx.name_of(t);
                    self.cx.error(
                        codes::NOT_CALLABLE,
                        callee.span,
                        format!("`{n}` is not callable"),
                        "not a function",
                    );
                }
                let ids = args.iter().map(|a| self.expr(a, None).id).collect();
                (Ty::Error, ids)
            }
        };
        let callee = hir::Callee::Value(c.id);
        self.mk(e, ty, hir::ExprKind::Call { callee, args: arg_ids })
    }

    fn require_mutable_receiver(&mut self, base: &Expr, base_ty: &Ty, method: &str) {
        let (ok, help) = match base_ty {
            Ty::Ref { mutable, .. } => (*mutable, None),
            Ty::Ptr { mutable, .. } | Ty::OwnPtr { mutable, .. } => (
                *mutable,
                Some(
                    "the method takes `&mut self`, and a `^T` is only read through — \
                     take a `^mut T` (spec §5.2)",
                ),
            ),
            _ => {
                let ok = self.place_mutability(base).unwrap_or(true);
                if ok {
                    self.require_unsafe_static_write(base);
                }
                (ok, None)
            }
        };
        if !ok {
            self.cx.error_help(
                codes::NOT_MUTABLE,
                base.span,
                format!("`{method}` needs a mutable receiver"),
                "this is not mutable",
                help.unwrap_or(
                    "the method takes `&mut self` — bind with `let mut`, or take `&mut` here \
                     (spec §7.3)",
                ),
            );
        }
    }

    /// Check a call's arguments against a signature, returning the nodes built for them.
    fn check_args(
        &mut self,
        sig: &FnSig,
        args: &[Expr],
        span: Span,
        name: &str,
    ) -> Vec<hir::ExprId> {
        if sig.variadic {
            return self.check_variadic_args(sig, args, span, name);
        }
        let required = sig.required();
        let total = sig.params.len();
        if args.len() < required || args.len() > total {
            let got = args.len();
            let expected = if required == total {
                format!("{required} {}", plural(required, "argument"))
            } else {
                format!("{required} to {total} arguments")
            };
            self.cx.error(
                codes::WRONG_ARG_COUNT,
                span,
                format!("`{name}` expects {expected}, found {got}"),
                "wrong number of arguments",
            );
        }
        let mut ids = Vec::with_capacity(args.len());
        for (a, p) in args.iter().zip(&sig.params) {
            let want = self.subst(&p.ty);
            let c = self.expr(a, Some(&want));
            self.expect(&want, &c.ty, a.span);
            ids.push(c.id);
        }
        for a in args.iter().skip(sig.params.len()) {
            ids.push(self.expr(a, None).id);
        }
        ids
    }

    /// A call of `fn f(a: A, xs: ..T)` (spec §7.2): the fixed arguments as usual, then every
    /// remaining argument a `T`, gathered into one array on the caller's stack that the
    /// parameter views as a `[]T`. `f(a, ..xs)` passes the slice `xs` itself instead. So the
    /// call always has exactly one argument per parameter, and nothing past checking sees a
    /// variadic call as anything but an array passed where a slice is wanted.
    fn check_variadic_args(
        &mut self,
        sig: &FnSig,
        args: &[Expr],
        span: Span,
        name: &str,
    ) -> Vec<hir::ExprId> {
        let fixed = sig.params.len() - 1;
        let required = sig.params[..fixed].iter().filter(|p| !p.has_default).count();
        let supplied_fixed = args.len().min(fixed);
        if args.len() < required {
            let got = args.len();
            self.cx.error(
                codes::WRONG_ARG_COUNT,
                span,
                format!(
                    "`{name}` expects at least {required} {}, found {got}",
                    plural(required, "argument")
                ),
                "wrong number of arguments",
            );
        }
        let mut ids = Vec::with_capacity(sig.params.len());
        for (a, p) in args.iter().zip(&sig.params[..fixed]) {
            let want = self.subst(&p.ty);
            let c = self.expr(a, Some(&want));
            self.expect(&want, &c.ty, a.span);
            ids.push(c.id);
        }
        // Defaulted fixed parameters the call left out are filled in by codegen as for any
        // call; the variadic slice after them has to be passed, so a call that gives no
        // variadic arguments cannot also skip a default in front of it.
        if supplied_fixed < fixed {
            if ids.len() < required {
                return ids;
            }
            self.cx.error_help(
                codes::BAD_VARIADIC,
                span,
                format!("`{name}` needs its defaulted arguments written out here"),
                "defaults before a variadic parameter",
                "a default in front of `..` cannot be skipped: pass it, and then the variadic \
                 arguments (spec §7.2)",
            );
            return ids;
        }

        let slice_ty = self.subst(&sig.params[fixed].ty);
        let elem = match &slice_ty {
            Ty::Slice { elem, .. } => (**elem).clone(),
            _ => Ty::Error,
        };
        let rest = &args[fixed..];
        // `f(..xs)`: the caller's slice, passed as it is.
        if let [only] = rest {
            if let Some(inner) = spread(only) {
                let c = self.expr(inner, Some(&slice_ty));
                if !c.ty.is_wildcard()
                    && !matches!(c.ty.peeled_refs(), Ty::Slice { .. } | Ty::Array { .. })
                {
                    let n = self.cx.name_of(&c.ty);
                    self.cx.error_help(
                        codes::BAD_VARIADIC,
                        only.span,
                        format!("`..` spreads a slice or an array, not `{n}`"),
                        "not a sequence",
                        "pass the values one by one, or spread a `[]T` (spec §7.2)",
                    );
                } else {
                    self.expect(&slice_ty, c.ty.peeled_refs(), inner.span);
                }
                ids.push(c.id);
                return ids;
            }
        }
        let mut elems = Vec::with_capacity(rest.len());
        for a in rest {
            if spread(a).is_some() {
                self.cx.error_help(
                    codes::BAD_VARIADIC,
                    a.span,
                    "a spread must be the only variadic argument",
                    "spread beside other arguments",
                    format!(
                        "`{name}(..xs)` passes a slice as it is; to add values, build the slice \
                         you want first (spec §7.2)"
                    ),
                );
                if let Some(inner) = spread(a) {
                    let _ = self.expr(inner, None);
                }
                continue;
            }
            elems.push(self.variadic_element(a, &elem));
        }
        let array_ty = if elems.is_empty() {
            slice_ty.clone()
        } else {
            Ty::Array { len: elems.len() as u64, elem: Rc::new(elem.clone()) }
        };
        let at = rest.first().map_or(span, |f| Span { start: f.span.start, end: span.end });
        ids.push(self.hir.expr(at, array_ty, hir::ExprKind::ArrayLit(elems)).id);
        ids
    }

    /// One argument of a variadic call, checked against its element type. A value given
    /// where a read-only trait object is wanted is borrowed for the call — `log("{}", hp)`
    /// with `args: ..^Format` refers to `hp` — since arguments of different types can only
    /// share a slice through pointers (spec §7.2, §7.6).
    fn variadic_element(&mut self, a: &Expr, elem: &Ty) -> hir::ExprId {
        let object = match elem {
            Ty::Ptr { mutable: false, inner } | Ty::Ref { mutable: false, inner } => {
                matches!(&**inner, Ty::Named { sym, .. }
                    if self.cx.res.symbol(*sym).kind == SymbolKind::Trait)
            }
            _ => false,
        };
        if object && !matches!(a.kind, ExprKind::Ref { .. }) {
            let c = self.expr(a, None);
            let is_pointer = matches!(c.ty, Ty::Ptr { .. } | Ty::OwnPtr { .. } | Ty::Ref { .. });
            if !is_pointer && !c.ty.is_wildcard() {
                let borrowed = Ty::Ref { mutable: false, inner: Rc::new(c.ty.defaulted()) };
                self.expect(elem, &borrowed, a.span);
                return self
                    .hir
                    .expr(a.span, borrowed, hir::ExprKind::Ref { mutable: false, place: c.id })
                    .id;
            }
            self.expect(elem, &c.ty, a.span);
            return c.id;
        }
        let c = self.expr(a, Some(elem));
        self.expect(elem, &c.ty, a.span);
        c.id
    }

    /// The declaration a path's *owner* segment names: the item itself for `f`, and the
    /// type or module member for `Type::Variant` and `module::f`.
    /// `Head::query` where `Head` names a type (spec §5.5). `None` when `Head` is not a type,
    /// or is a type whose own member of that name should win.
    fn path_type_query(&mut self, head: &PathSegment, q: &Ident) -> Option<(Ty, hir::ExprKind)> {
        let name = &head.name.name;
        let is_param = self.cx.param_in_scope(name).is_some();
        let is_builtin = crate::builtin_type(name).is_some();
        if !is_param && !is_builtin {
            // A declared type answers only if it has no member of that name.
            let sym = self.cx.res.lookup_in_file(self.cx.module, self.cx.file, name)?;
            let symbol = self.cx.res.symbol(sym);
            let is_type =
                matches!(symbol.kind, SymbolKind::Struct | SymbolKind::Union | SymbolKind::Enum);
            if !is_type
                || symbol.member(&q.name).is_some()
                || hir::TypeQuery::from_name(&q.name).is_none()
            {
                return None;
            }
        }
        let path = Path { span: head.span, segments: vec![head.clone()] };
        let ty = self.cx.lower_type(&Type { span: head.span, kind: TypeKind::Path(path) });
        let ty = self.subst(&ty);
        match hir::TypeQuery::from_name(&q.name) {
            Some(query) => Some((query.result(), hir::ExprKind::TypeQuery { ty, query })),
            None => {
                let shown = self.cx.name_of(&ty);
                let suggestion =
                    dz_resolve::closest(&q.name, dz_resolve::TYPE_QUERIES.iter().copied());
                let mut d = Diagnostic::error(
                    codes::NO_SUCH_FIELD,
                    format!("`{shown}` has no compile-time query `{}`", q.name),
                )
                .at(self.cx.file, q.span, "unknown query");
                d = d.with_help(match suggestion {
                    Some(s) => format!("did you mean `{s}`? (spec §5.5)"),
                    None => format!(
                        "a type answers {} (spec §5.5)",
                        dz_resolve::TYPE_QUERIES
                            .iter()
                            .map(|q| format!("`{q}`"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                });
                self.cx.diags.push(d);
                Some((Ty::Error, hir::ExprKind::Error))
            }
        }
    }

    /// §12.9: a formatting call's first argument is a string literal, parsed here, so a
    /// wrong argument count, an unknown `{:spec}`, or an argument with no way to be written
    /// is an ordinary compile error pointing into the literal — not a stray `{}` at runtime
    /// and not a `printf` mismatch.
    fn check_format(&mut self, name: &str, args: &[Expr], checked: &[Checked], call: Span) {
        let at = match name {
            "println" | "print" | "eprintln" | "eprint" | "format" | "panic" => 0,
            // `error(code?, fmt, args…)`: the code is optional and comes first (§9.1).
            "error" => usize::from(
                checked.len() > 1
                    && (checked[0].ty.is_integral() || matches!(checked[0].ty, Ty::Named { .. })),
            ),
            _ => return,
        };
        let Some(fmt) = args.get(at) else {
            self.cx.error_help(
                codes::BAD_FORMAT_STRING,
                call,
                format!("`{name}` needs a format string"),
                "no format string",
                format!("write `{name}(\"…\")` (spec §12.9)"),
            );
            return;
        };
        let raw = match &fmt.kind {
            ExprKind::Str(t) => t.strip_prefix('"').and_then(|t| t.strip_suffix('"')),
            ExprKind::RawStr(t) => t.strip_prefix('`').and_then(|t| t.strip_suffix('`')),
            _ => None,
        };
        let Some(raw) = raw else {
            self.cx.error_help(
                codes::BAD_FORMAT_STRING,
                fmt.span,
                "the format string must be a string literal",
                "not a literal",
                "it is parsed at compile time, which is what makes a mismatch an error rather \
                 than garbage — write `println(\"{}\", value)` (spec §12.9)",
            );
            return;
        };
        // Offsets inside the literal map straight onto the file: the AST keeps the exact
        // source text, and no escape sequence contains a brace.
        let base = fmt.span.start + 1;
        let span_of = |s: usize, e: usize| Span { start: base + s as u32, end: base + e as u32 };

        let bytes = raw.as_bytes();
        let mut holes: Vec<(usize, usize, &str)> = Vec::new();
        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'{' if bytes.get(i + 1) == Some(&b'{') => i += 2,
                b'}' if bytes.get(i + 1) == Some(&b'}') => i += 2,
                b'{' => {
                    let Some(rel) = raw[i + 1..].find('}') else {
                        self.cx.error_help(
                            codes::BAD_FORMAT_STRING,
                            span_of(i, bytes.len()),
                            "unclosed `{` in a format string",
                            "this `{` is never closed",
                            "write `{{` for a literal brace (spec §12.9)",
                        );
                        return;
                    };
                    holes.push((i, i + 2 + rel, &raw[i + 1..i + 1 + rel]));
                    i += 2 + rel;
                }
                b'}' => {
                    self.cx.error_help(
                        codes::BAD_FORMAT_STRING,
                        span_of(i, i + 1),
                        "unmatched `}` in a format string",
                        "nothing opened this",
                        "write `}}` for a literal brace (spec §12.9)",
                    );
                    return;
                }
                _ => i += 1,
            }
        }

        let mut values = &checked[(at + 1).min(checked.len())..];
        // `panic("…", args…, loc)`: a final `SourceLocation` is where the panic reports it
        // happened, not something to write — which is how `assert` names its caller's line
        // (spec §9.8, §12.7). It cannot be mistaken for a value, since it has no format.
        if name == "panic" && values.last().is_some_and(|v| is_source_location(&v.ty)) {
            values = &values[..values.len() - 1];
        }
        for (k, (s, e, inner)) in holes.iter().enumerate() {
            let here = span_of(*s, *e);
            let spec = if inner.is_empty() {
                ""
            } else if let Some(sp) = inner.strip_prefix(':') {
                sp
            } else {
                self.cx.error_help(
                    codes::BAD_FORMAT_STRING,
                    here,
                    "format strings do not interpolate names",
                    "not a placeholder",
                    format!(
                        "write `{{}}` and pass `{inner}` as an argument — interpolation is \
                         deliberately absent (spec §12.9)"
                    ),
                );
                continue;
            };
            let Some(v) = values.get(k) else { continue };
            let t = self.subst(&v.ty).defaulted();
            // An argument already reported has nothing more to say about itself.
            if !t.is_wildcard() && !formattable(&t) {
                let shown = self.cx.name_of(&t);
                let help = if matches!(&t, Ty::Named { sym, .. }
                    if self.cx.res.symbol(*sym).kind == SymbolKind::Enum)
                {
                    "an enum is written through its backing integer: `x as i32` (spec §7.4)"
                        .to_owned()
                } else {
                    "numbers, `bool`, `char`, `str`, `cstr` and pointers can be formatted; a \
                     type's own `Format` implementation arrives with the stdlib (spec §12.9, \
                     §19 M12) — format the fields you mean"
                        .to_owned()
                };
                self.cx.error_help(
                    codes::NOT_FORMATTABLE,
                    args[at + 1 + k].span,
                    format!("`{shown}` cannot be formatted"),
                    "no way to write this",
                    help,
                );
                continue;
            }
            if let Err(why) = spec_fits(spec, &t) {
                let shown = self.cx.name_of(&t);
                self.cx.error_help(
                    codes::BAD_FORMAT_STRING,
                    here,
                    format!("`{{:{spec}}}` does not suit `{shown}`"),
                    why,
                    "`{:x}` and `{:X}` write an integer in hex, `{:.N}` a float with N \
                     decimals (spec §12.9)",
                );
            }
        }

        if holes.len() != values.len() {
            let (h, v) = (holes.len(), values.len());
            self.cx.error_help(
                codes::BAD_FORMAT_STRING,
                fmt.span,
                format!(
                    "this format string has {h} {} but {v} {} given",
                    plural(h, "placeholder"),
                    if v == 1 { "argument was" } else { "arguments were" }
                ),
                format!("{h} `{{}}` here"),
                "each `{}` takes one argument, in order; write `{{` or `}}` for a literal \
                 brace (spec §12.9)",
            );
        }
    }

    /// `sizeof(T)` / `alignof(T)`: the argument is a type, so it is lowered as one.
    /// `typeinfo_of<T>()`, `typeid_of<T>()` and `typeinfo(id)` (spec §13.1). The first two
    /// name a type and take nothing; the third takes a `TypeId`. Each answers from the
    /// standard library's `TypeInfo` and `TypeId`, and needs a type table to answer from.
    fn reflect_call(
        &mut self,
        e: &Expr,
        name: &str,
        seg: &PathSegment,
        args: &[Expr],
        span: Span,
    ) -> Checked {
        let std = self.cx.res.prelude;
        let (Some(info_sym), Some(id_sym)) =
            (self.cx.res.lookup(std, "TypeInfo"), self.cx.res.lookup(std, "TypeId"))
        else {
            return self.mk(e, Ty::Error, hir::ExprKind::Error);
        };
        let info_ty = Ty::reference(false, Ty::plain(info_sym));
        if self.cx.ws.module(self.cx.ws.root).build.type_info == dz_project::TypeInfo::None {
            self.cx.error_help(
                codes::REFLECTION_DISABLED,
                span,
                format!("`{name}` needs the type table, and this build has none"),
                "reflection is turned off",
                "set `type_info = \"used\"` under `[build]` in dazzle.toml (spec §13.2)",
            );
            for a in args {
                self.expr(a, None);
            }
            return self.mk(e, Ty::Error, hir::ExprKind::Error);
        }
        if name == "typeinfo" {
            let want = Ty::plain(id_sym);
            if args.len() != 1 || !seg.generics.is_empty() {
                self.cx.error(
                    codes::WRONG_ARG_COUNT,
                    span,
                    format!("`typeinfo` takes one `TypeId`, found {} arguments", args.len()),
                    "write `typeinfo(id)`, or `typeinfo_of<T>()` for a type you can name",
                );
                for a in args {
                    self.expr(a, None);
                }
                return self.mk(e, Ty::Error, hir::ExprKind::Error);
            }
            let c = self.expr(&args[0], Some(&want));
            self.expect(&want, &c.ty, args[0].span);
            let kind = hir::ExprKind::Builtin { name: "typeinfo".to_owned(), args: vec![c.id] };
            return self.mk(e, info_ty, kind);
        }
        // `typeinfo_of<T>()`: one type argument, no value arguments.
        if seg.generics.len() != 1 || !args.is_empty() {
            self.cx.error_help(
                codes::WRONG_ARG_COUNT,
                span,
                format!("`{name}` takes one type and no arguments"),
                "not the form this call has",
                format!("write `{name}<T>()` (spec §13.1)"),
            );
            for a in args {
                self.expr(a, None);
            }
            return self.mk(e, Ty::Error, hir::ExprKind::Error);
        }
        let ty = self.cx.lower_type(&seg.generics[0]);
        let ty = self.subst(&ty);
        let info = name == "typeinfo_of";
        let result = if info { info_ty } else { Ty::plain(id_sym) };
        self.mk(e, result, hir::ExprKind::Reflect { ty, info })
    }

    fn type_argument_query(&mut self, name: &str, arg: &Expr) -> Option<(Ty, hir::ExprKind)> {
        let ExprKind::Path(tp) = &arg.kind else { return None };
        // A value is not a type: `sizeof(x)` for a local `x` is a mistake worth naming,
        // since it would otherwise lower to nothing and be accepted in silence.
        if tp.segments.len() == 1 && self.local(&tp.segments[0].name.name).is_some() {
            let v = tp.segments[0].name.name.clone();
            let ty = self.local(&v).map(|l| l.ty.clone()).unwrap_or(Ty::Error);
            let ty = self.cx.name_of(&ty);
            self.cx.error_help(
                codes::TYPE_MISMATCH,
                arg.span,
                format!("`{name}` takes a type, and `{v}` is a value"),
                "not a type",
                format!("`{v}` is a `{ty}`, so write `{name}({ty})` (spec §5.5)"),
            );
            return Some((Ty::Error, hir::ExprKind::Error));
        }
        let ty = self.cx.lower_type(&Type { span: tp.span, kind: TypeKind::Path(tp.clone()) });
        let ty = self.subst(&ty);
        let query = if name == "sizeof" { hir::TypeQuery::Size } else { hir::TypeQuery::Align };
        Some((query.result(), hir::ExprKind::TypeQuery { ty, query }))
    }

    fn path_symbol(&self, p: &Path) -> Option<SymbolId> {
        self.path_head(p).map(|(sym, _)| sym)
    }

    /// The symbol a path leads to before its members, and how many segments reaching it
    /// took: one for a name in scope, two through a module alias, and one more for each
    /// C++ namespace walked through on the way (spec §15.2).
    fn path_head(&self, p: &Path) -> Option<(SymbolId, usize)> {
        // Modules first — `rl::shapes::Rectangle` through a public dependency (spec §3.2) —
        // then `Layer::Ui`, whose leading segment is a type in scope. Without the second the
        // path resolved to nothing and, because "nothing" types as `Ty::Error`, it was
        // accepted in silence rather than reported (spec §3.3).
        let names: Vec<&str> = p.segments.iter().map(|s| s.name.name.as_str()).collect();
        self.cx.res.path_item(self.cx.module, self.cx.file, &names)
    }

    fn path_expr(&mut self, p: &Path, e: &Expr, expected: Option<&Ty>) -> Checked {
        let Some(first) = p.segments.first() else {
            return self.mk(e, Ty::Error, hir::ExprKind::Error);
        };

        // `T::is_trivially_copyable`, `i32::size`, `Vec3::is_copyable` — a compile-time
        // query about a type rather than a path to a value (spec §5.5).
        if p.segments.len() == 2 {
            if let Some((ty, kind)) = self.path_type_query(first, &p.segments[1].name) {
                return self.mk(e, ty, kind);
            }
        }

        // A local wins over everything for a bare name.
        if p.segments.len() == 1 {
            if let Some(l) = self.local(&first.name.name).cloned() {
                let kind = match l.slot {
                    Slot::Local(id) => hir::ExprKind::Local(id),
                    // An unqualified field in a method body is `self.field` (spec §7.3).
                    Slot::SelfField { index } => match self.self_local {
                        Some(base) => {
                            let self_ty = self.self_local_ty.clone().unwrap_or(Ty::Error);
                            let base = self.hir.expr(e.span, self_ty, hir::ExprKind::Local(base));
                            hir::ExprKind::Field { base: base.id, index }
                        }
                        None => hir::ExprKind::Error,
                    },
                };
                return self.mk(e, l.ty.clone(), kind);
            }
            if dz_resolve::BUILTIN_FNS.contains(&first.name.name.as_str()) {
                // Only meaningful when called; `call` handles that before reaching here.
                return self.mk(e, Ty::Error, hir::ExprKind::Error);
            }
            // `cstr("x")`, `i32(3)`: a builtin type is not a function, and conversion is
            // `as` (spec §10.6). Without this the name went silently wrong, and the only
            // message left was a later one about whatever consumed the result.
            let name = first.name.name.as_str();
            if crate::builtin_type(name).is_some() {
                let help = if name == "cstr" {
                    "a string literal is a `cstr` wherever one is wanted, or write `\"…\" as cstr` \
                     (spec §5.3)"
                        .to_owned()
                } else {
                    format!("convert with `as`: `x as {name}` (spec §10.6)")
                };
                self.cx.error_help(
                    codes::NOT_CALLABLE,
                    first.name.span,
                    format!("`{name}` is a type, not a function or a value"),
                    "a type",
                    help,
                );
                return self.mk(e, Ty::Error, hir::ExprKind::Error);
            }
        }

        let Some((sym, used)) = self.path_head(p) else {
            return self.mk(e, Ty::Error, hir::ExprKind::Error);
        };
        let symbol_kind = self.cx.res.symbol(sym).kind;

        // `Type::Variant`, `module::Type::Variant`, `header::ns::Type::Variant`
        let tail = &p.segments[used.min(p.segments.len())..];

        if let Some(seg) = tail.first() {
            let (ty, kind) = self.assoc_item(sym, seg, p, expected);
            return self.mk(e, ty, kind);
        }

        let (ty, kind) = match symbol_kind {
            SymbolKind::Fn => match self.cx.table.get(sym).cloned() {
                Some(ItemSig::Fn(sig)) => {
                    let seg = &p.segments[used.clamp(1, p.segments.len()) - 1];
                    let type_args = self.fn_value_type_args(&sig, seg, expected, e.span);
                    let map: HashMap<u32, Ty> = sig
                        .generics
                        .iter()
                        .map(|g| g.param.id)
                        .zip(type_args.iter().cloned())
                        .collect();
                    (
                        Ty::Fn {
                            params: sig
                                .params
                                .iter()
                                .map(|p| ty::substitute(&p.ty, &map))
                                .collect(),
                            ret: Rc::new(ty::substitute(&sig.ret, &map)),
                        },
                        hir::ExprKind::FnItem { sym, type_args },
                    )
                }
                _ => (Ty::Error, hir::ExprKind::Error),
            },
            SymbolKind::Const => match self.cx.table.get(sym) {
                Some(ItemSig::Const(t)) => (t.clone(), hir::ExprKind::Const(sym)),
                _ => (Ty::Error, hir::ExprKind::Error),
            },
            SymbolKind::Static => match self.cx.table.get(sym) {
                Some(ItemSig::Static(t)) => (t.clone(), hir::ExprKind::Static(sym)),
                _ => (Ty::Error, hir::ExprKind::Error),
            },
            // A bare type name in value position is only meaningful as a constructor
            // call, which `call` handles before reaching here.
            _ => (Ty::plain(sym), hir::ExprKind::Error),
        };
        self.mk(e, ty, kind)
    }

    fn assoc_item(
        &mut self,
        owner: SymbolId,
        seg: &PathSegment,
        path: &Path,
        expected: Option<&Ty>,
    ) -> (Ty, hir::ExprKind) {
        match self.cx.table.get(owner) {
            Some(ItemSig::Enum { variants, .. }) => {
                match variants.iter().position(|v| v.name == seg.name.name) {
                    Some(index) => (Ty::plain(owner), hir::ExprKind::EnumVariant { owner, index }),
                    // Name resolution already reported the unknown variant.
                    None => (Ty::Error, hir::ExprKind::Error),
                }
            }
            Some(ItemSig::Struct(st)) if st.consts.iter().any(|c| c.name == seg.name.name) => {
                let c = st.consts.iter().find(|c| c.name == seg.name.name).cloned();
                let Some(c) = c else { return (Ty::Error, hir::ExprKind::Error) };
                self.check_member_visible(owner, &c.name, c.vis, c.span, seg.name.span, "constant");
                (c.ty, hir::ExprKind::AssocConst { owner, name: c.name })
            }
            Some(ItemSig::Union { variants }) => {
                match variants.iter().position(|v| v.name == seg.name.name) {
                    Some(index) => {
                        let carries = variants[index].carries.is_some();
                        let args = self.variant_args(path, owner, expected, None);
                        // `ButtonState::Idle` with no braces builds a carried struct from
                        // its defaults, so a field without one is as missing as in `Idle{}`.
                        let kind = if carries {
                            self.union_variant_literal(owner, &args, index, &[], seg.name.span)
                        } else {
                            hir::ExprKind::UnionVariant { owner, index, payload: Vec::new() }
                        };
                        (Ty::Named { sym: owner, args }, kind)
                    }
                    None => (Ty::Error, hir::ExprKind::Error),
                }
            }
            _ => (Ty::Error, hir::ExprKind::Error),
        }
    }

    /// Check a struct literal and build its node. Values come back in *declaration*
    /// order, with `None` where the field's declared default applies, so no consumer has
    /// to consult the declaration to know what order to emit initialisers in.
    /// Building a value of a type with a body-less `virtual fn`: there is nothing for the
    /// vtable slot to point at, so no such value can exist (spec §7.3, 18.32).
    fn refuse_abstract(&mut self, sym: SymbolId, span: Span) {
        let Some((owner, method)) = self.cx.table.abstract_method(sym) else { return };
        let name = self.cx.res.symbol(sym).name.clone();
        let owner = self.cx.res.symbol(owner).name.clone();
        let help = if owner == name {
            format!(
                "give `{method}` a body, or build a struct that inherits from `{name}` \
                 and overrides it (spec §7.3)"
            )
        } else {
            format!(
                "`{name}` inherits `{owner}::{method}` and does not override it — add \
                 `override fn {method}(…) {{ … }}` to `{name}` (spec §7.3)"
            )
        };
        self.cx.error_help(
            codes::ABSTRACT_TYPE,
            span,
            format!("`{name}` is abstract and cannot be built"),
            format!("`{owner}::{method}` has no body"),
            help,
        );
    }

    fn struct_literal(&mut self, ty: &Ty, fields: &[FieldInit], span: Span) -> hir::ExprKind {
        let Some(sym) = ty.named() else {
            for f in fields {
                self.expr(&f.value, None);
            }
            return hir::ExprKind::Error;
        };
        let Some(sig) = self.cx.table.struct_sig(sym).cloned() else {
            for f in fields {
                self.expr(&f.value, None);
            }
            return hir::ExprKind::Error;
        };
        // A literal of an instantiation checks its fields against the instantiated types:
        // `Pair<f32>{ first: 1.5 }` wants an `f32`, not the `T` the declaration wrote
        // (spec §8.4). Empty arguments leave a plain struct's fields as they are.
        let args = match ty {
            Ty::Named { args, .. } => args.clone(),
            _ => Vec::new(),
        };
        let instantiated: Vec<crate::FieldSig> = sig
            .fields
            .iter()
            .zip(self.cx.table.field_types(sym, &args, &sig.fields))
            .map(|(f, t)| crate::FieldSig { ty: t, ..f.clone() })
            .collect();
        let owner = self.cx.res.symbol(sym).name.clone();
        self.refuse_abstract(sym, span);
        // A type with a constructor is built through it, never field by field: the
        // constructor is what establishes whatever invariant the type has, and a literal
        // would step around it. A base's constructor counts, since building the derived
        // type builds the base (spec §7.2, §7.3, 18.32). An imported C or C++ struct is
        // exempt: its "constructor" is the C++ one, and a C struct's literal is how §15.2
        // says to write one.
        if !sig.is_union && self.cx.ws.module(self.cx.res.symbol(sym).module).foreign.is_none() {
            let with_ctor = self
                .cx
                .table
                .with_bases(sym)
                .into_iter()
                .find(|s| self.cx.table.struct_sig(*s).is_some_and(|x| !x.ctors.is_empty()));
            if let Some(c) = with_ctor {
                let cname = self.cx.res.symbol(c).name.clone();
                let help = if c == sym {
                    format!("call it: `{owner}(…)` (spec §7.2)")
                } else {
                    format!(
                        "`{owner}` inherits from `{cname}`, which has one, so a `{owner}` \
                         is built through a constructor of its own — give `{owner}` one, \
                         written `{owner}(…) : {cname}(…) {{ … }}` (spec §7.3)"
                    )
                };
                self.cx.error_help(
                    codes::MISSING_BASE_INIT,
                    span,
                    format!("`{owner}` has a constructor, so it is not built with a literal"),
                    format!("`{cname}` declares a constructor"),
                    help,
                );
            }
        }
        // An imported C union's members are one piece of memory, so writing two of them
        // writes the same bytes twice and only the last survives (spec §15.2, §18.41).
        if sig.is_union && fields.len() > 1 {
            self.cx.error_help(
                codes::UNION_LITERAL,
                span,
                format!("`{owner}` is a C union, so this sets {} of its members", fields.len()),
                "they are the same memory",
                "set the one you mean; the others are whatever those bytes then hold \
                 (spec §15.2)",
            );
        }
        let values = self.field_values(&instantiated, Some(sym), &owner, fields, span);
        hir::ExprKind::StructLit { ty: sym, fields: values }
    }

    /// Check a brace-initialiser against a declared field list, returning one slot per
    /// field in **declaration** order with `None` where the declared default applies.
    ///
    /// Shared by struct literals and union-variant literals, which differ only in where
    /// the field list comes from (spec §7.3, §7.5).
    fn field_values(
        &mut self,
        decl: &[crate::FieldSig],
        vis_owner: Option<SymbolId>,
        owner: &str,
        fields: &[FieldInit],
        span: Span,
    ) -> Vec<Option<hir::ExprId>> {
        let decl = decl.to_vec();
        let mut values: Vec<Option<hir::ExprId>> = vec![None; decl.len()];

        let positional = fields.iter().all(|f| f.name.is_none()) && !fields.is_empty();
        if positional {
            // `Vec3{1, 2, 3}` is allowed only when every field is listed (spec §7.3).
            if fields.len() != decl.len() {
                let (want, got) = (decl.len(), fields.len());
                self.cx.error_help(
                    codes::MISSING_FIELD,
                    span,
                    format!("`{owner}` has {want} fields, found {got}"),
                    "wrong number of values",
                    "positional initialisation requires every field; use `name: value` \
                     otherwise (spec §7.3)",
                );
            }
            for (i, (f, d)) in fields.iter().zip(&decl).enumerate() {
                let want = self.subst(&d.ty);
                let c = self.expr(&f.value, Some(&want));
                self.expect(&want, &c.ty, f.value.span);
                values[i] = Some(c.id);
            }
            return values;
        }

        let mut seen: Vec<&str> = Vec::new();
        for f in fields {
            let Some(name) = &f.name else {
                self.expr(&f.value, None);
                continue;
            };
            let found = decl.iter().position(|x| x.name == name.name);
            let Some(field) = found.map(|i| &decl[i]) else {
                let names: Vec<String> = decl.iter().map(|x| x.name.clone()).collect();
                let suggestion = dz_resolve::closest(&name.name, names.iter().map(String::as_str));
                let mut d = dz_diag::Diagnostic::error(
                    codes::NO_SUCH_FIELD,
                    format!("`{owner}` has no field `{}`", name.name),
                )
                .at(self.cx.file, name.span, "unknown field");
                if let Some(s) = suggestion {
                    d = d.with_help(format!("did you mean `{s}`?"));
                }
                self.cx.diags.push(d);
                self.expr(&f.value, None);
                continue;
            };
            if seen.contains(&name.name.as_str()) {
                self.cx.error(
                    codes::DUPLICATE_FIELD,
                    name.span,
                    format!("field `{}` is initialised twice", name.name),
                    "duplicate initialiser",
                );
            }
            seen.push(&name.name);
            let (decl_name, decl_vis, decl_span, decl_ty) =
                (field.name.clone(), field.vis, field.span, field.ty.clone());
            if let Some(sym) = vis_owner {
                self.check_member_visible(sym, &decl_name, decl_vis, decl_span, name.span, "field");
            }
            let want = self.subst(&decl_ty);
            let c = self.expr(&f.value, Some(&want));
            self.expect(&want, &c.ty, f.value.span);
            if let Some(i) = found {
                values[i] = Some(c.id);
            }
        }

        // Anything omitted must have a default (spec §6.2, §7.3).
        let missing: Vec<String> = decl
            .iter()
            .filter(|d| !d.has_default && !seen.contains(&d.name.as_str()))
            .map(|d| format!("`{}`", d.name))
            .collect();
        if !missing.is_empty() {
            let list = missing.join(", ");
            self.cx.error_help(
                codes::MISSING_FIELD,
                span,
                format!(
                    "missing {} in `{owner}`",
                    if missing.len() == 1 { "field" } else { "fields" }
                ),
                format!("{list} not initialised"),
                "a field with a declared default may be omitted (spec §6.2)",
            );
        }
        values
    }

    /// `Shape::Circle{ radius: 2 }` — a union variant's payload, which is a field list of
    /// the variant's own rather than of any struct (spec §7.5).
    fn union_variant_literal(
        &mut self,
        owner: SymbolId,
        args: &[Ty],
        index: usize,
        fields: &[FieldInit],
        span: Span,
    ) -> hir::ExprKind {
        let variant = match self.cx.table.get(owner) {
            Some(ItemSig::Union { variants }) => variants.get(index).cloned(),
            _ => None,
        };
        let Some(variant) = variant else { return hir::ExprKind::Error };
        // A variant carrying a struct is built as that struct is: its fields, its defaults,
        // its visibility, and none of it when it has a constructor (spec §7.5, 18.45).
        if let Some(carried) = variant.carries {
            return match self.struct_literal(&Ty::plain(carried), fields, span) {
                hir::ExprKind::StructLit { fields: payload, .. } => {
                    hir::ExprKind::UnionVariant { owner, index, payload }
                }
                _ => hir::ExprKind::Error,
            };
        }
        let name = format!("{}::{}", self.cx.res.symbol(owner).name, variant.name);
        // A `Maybe<i32>::Some{ value: 4 }` checks `value` against `i32`, not `T` (spec §8.4).
        let instantiated: Vec<crate::FieldSig> = variant
            .fields
            .iter()
            .zip(self.cx.table.field_types(owner, args, &variant.fields))
            .map(|(f, t)| crate::FieldSig { ty: t, ..f.clone() })
            .collect();
        // Visibility is the union's, not a per-field thing: a variant's fields are as
        // public as the variant is, so nothing is checked per field here.
        let payload = self.field_values(&instantiated, None, &name, fields, span);
        hir::ExprKind::UnionVariant { owner, index, payload }
    }

    /// The type arguments a variant path gives its union: `Maybe<i32>::Some` says them,
    /// `let m: Maybe<i32> = Maybe::Nothing` takes them from what is expected, and inside
    /// `Maybe<T>`'s own body a bare `Maybe` is `Maybe<T>`. Failing those, the payload says
    /// them, as a generic call's arguments do: `Choice::One{ value: 4 }` is a `Choice<i32>`
    /// (spec §8.1). A variant with nothing to infer from is DZ0534.
    fn variant_args(
        &mut self,
        p: &Path,
        owner: SymbolId,
        expected: Option<&Ty>,
        payload: Option<(usize, &[FieldInit])>,
    ) -> Vec<Ty> {
        let params: Vec<u32> =
            self.cx.table.type_params(owner).iter().map(|d| d.param.id).collect();
        if params.is_empty() || p.segments.len() < 2 {
            return Vec::new();
        }
        let seg = &p.segments[p.segments.len() - 2];
        if seg.generics.is_empty() {
            if let Some(Ty::Named { sym, args }) = expected.map(|t| self.subst(t)) {
                if sym == owner && !args.is_empty() && !args.iter().any(Ty::has_params) {
                    return args;
                }
            }
            if let Some(args) = self.infer_variant_args(owner, &params, payload) {
                return args;
            }
        }
        self.cx.type_args(owner, seg)
    }

    /// Solve a generic union's parameters from a variant literal's field values.
    fn infer_variant_args(
        &mut self,
        owner: SymbolId,
        params: &[u32],
        payload: Option<(usize, &[FieldInit])>,
    ) -> Option<Vec<Ty>> {
        let (index, fields) = payload?;
        let variant = match self.cx.table.get(owner) {
            Some(ItemSig::Union { variants }) => variants.get(index)?.clone(),
            _ => return None,
        };
        let mut map = HashMap::new();
        for (i, f) in fields.iter().enumerate() {
            let decl = match &f.name {
                Some(n) => variant.fields.iter().find(|d| d.name == n.name),
                None => variant.fields.get(i),
            };
            let Some(decl) = decl else { continue };
            let actual = self.expr_quiet(&f.value);
            unify(&decl.ty, &actual.defaulted(), &mut map);
        }
        params.iter().map(|id| map.get(id).cloned()).collect()
    }

    /// If `p` names a union variant, the union and the variant's position.
    fn union_variant_path(&self, p: &Path) -> Option<(SymbolId, usize)> {
        if p.segments.len() < 2 {
            return None;
        }
        let sym = self.path_symbol(p)?;
        let ItemSig::Union { variants } = self.cx.table.get(sym)? else {
            return None;
        };
        let last = &p.segments.last()?.name.name;
        let index = variants.iter().position(|v| v.name == *last)?;
        Some((sym, index))
    }

    /// `[1, 2, 3]` (spec §5.3).
    ///
    /// The element type comes from the annotation when there is one — including from a
    /// slice annotation, since a literal may be *passed* where a slice is wanted — so
    /// `[4]f32 = [1, 2, 3, 4]` gives four `f32`s rather than four `i32`s that would then
    /// need a conversion Dazzle does not have (§5.1). With no annotation the elements must
    /// agree with each other and an untyped one defaults as §2.5 says.
    fn array_literal(&mut self, elems: &[Expr], e: &Expr, expected: Option<&Ty>) -> Checked {
        let want = expected.map(|t| self.subst(t));
        let want_elem = match &want {
            Some(Ty::Array { elem, .. } | Ty::Slice { elem, .. }) => Some((**elem).clone()),
            _ => None,
        };
        let want_len = match &want {
            Some(Ty::Array { len, .. }) => Some(*len),
            _ => None,
        };

        let mut ids = Vec::new();
        let mut found: Option<Ty> = None;
        for x in elems {
            let c = self.expr(x, want_elem.as_ref());
            match &want_elem {
                Some(w) => self.expect(w, &c.ty, x.span),
                // No annotation: the first element sets the type and the rest must join
                // with it. One diagnostic per disagreeing element, at that element.
                None => {
                    found = match found {
                        None => Some(c.ty.clone()),
                        Some(prev) => match join(&prev, &c.ty) {
                            Some(j) => Some(j),
                            None => {
                                // Shown defaulted: an unannotated `[1, "two"]` is settling
                                // on `i32`, and `{integer}` would name a thing the user
                                // cannot write (spec §2.5).
                                let (a, b) = (
                                    self.cx.name_of(&prev.defaulted()),
                                    self.cx.name_of(&c.ty.defaulted()),
                                );
                                self.cx.error_help(
                                    codes::TYPE_MISMATCH,
                                    x.span,
                                    format!("this array's elements disagree: `{a}` and `{b}`"),
                                    format!("expected `{a}`, found `{b}`"),
                                    "every element of an array has the same type; annotate \
                                     the binding to say which (spec §5.3)",
                                );
                                Some(Ty::Error)
                            }
                        },
                    }
                }
            }
            ids.push(c.id);
        }

        let elem = want_elem.or(found).map(|t| t.defaulted()).unwrap_or_else(|| {
            // `[]` with nothing to read an element type from.
            self.cx.error_help(
                codes::NO_CONTEXT_FOR_LITERAL,
                e.span,
                "cannot infer the element type of an empty array",
                "no expected type here",
                "annotate the binding, e.g. `let a: [0]i32 = []` (spec §5.3)",
            );
            Ty::Error
        });

        // The length is exactly what is written (spec §5.3). On a mismatch the *expected*
        // length is reported back, so the outer check does not restate the same mistake.
        let len = match want_len {
            Some(n) if n != elems.len() as u64 => {
                let shown = self.cx.name_of(want.as_ref().expect("want_len implies want"));
                let got = elems.len();
                self.cx.error_help(
                    codes::TYPE_MISMATCH,
                    e.span,
                    format!("expected `{shown}`, found {got} {}", plural(got, "element")),
                    format!("this array has {got} {}", plural(got, "element")),
                    "an array literal lists exactly as many elements as its type has; \
                     `let mut a: [N]T` gives a zeroed one instead (spec §5.3, §6.2)",
                );
                n
            }
            _ => elems.len() as u64,
        };

        let ty = Ty::Array { len, elem: Rc::new(elem) };
        self.mk(e, ty, hir::ExprKind::ArrayLit(ids))
    }

    /// An array that is not a place cannot be *stored* as a slice (spec §5.3).
    ///
    /// The slice would point at a temporary that is gone by the end of the statement. Keyed
    /// on placehood rather than on the syntax, so it also catches an array coming out of an
    /// `if` or a call, and still allows slicing a local or a field — those outlive the
    /// binding. Storing sites call this; an argument does not, because a temporary passed to
    /// a call outlives the call.
    fn reject_temporary_slice(&mut self, target: &Ty, value: &Expr, value_ty: &Ty) {
        let (Ty::Slice { .. }, Ty::Array { .. }) = (&self.subst(target), &self.subst(value_ty))
        else {
            return;
        };
        if self.place_mutability(value).is_some() {
            return;
        }
        let shown = self.cx.name_of(target);
        self.cx.error_help(
            codes::NOT_MUTABLE,
            value.span,
            format!("cannot store a temporary array as `{shown}`"),
            "this array does not outlive the statement",
            "a slice is a view, so it would point at storage that is already gone — bind \
             the array first, then take a slice of the binding (spec §5.3)",
        );
    }

    fn dot_infer(
        &mut self,
        want: &Ty,
        name: &Ident,
        init: Option<&[FieldInit]>,
        span: Span,
    ) -> hir::ExprKind {
        let Some(sym) = want.named() else {
            // `.Variant` takes its variant from an enum or a union; an expected type that is
            // neither leaves nothing to infer from. Saying nothing here let the mistake
            // through in silence — `.NotAThing` where a `u32` was wanted checked clean.
            if !want.is_wildcard() {
                let shown = self.cx.name_of(want);
                self.cx.error_help(
                    codes::NO_SUCH_FIELD,
                    name.span,
                    format!("`{shown}` has no variants"),
                    format!("`.{}` needs an enum or a union", name.name),
                    format!(
                        "what is wanted here is a `{shown}`; write the value itself, or name \
                         the type it belongs to: `SomeEnum::{}` (spec §7.4)",
                        name.name
                    ),
                );
            }
            return hir::ExprKind::Error;
        };
        let is_enum = matches!(self.cx.table.get(sym), Some(ItemSig::Enum { .. }));
        let found = match self.cx.table.get(sym) {
            Some(ItemSig::Enum { variants, .. }) | Some(ItemSig::Union { variants }) => {
                variants.iter().position(|v| v.name == name.name)
            }
            _ => None,
        };
        if found.is_none() {
            let owner = self.cx.res.symbol(sym).name.clone();
            let names: Vec<String> = match self.cx.table.get(sym) {
                Some(ItemSig::Enum { variants, .. }) | Some(ItemSig::Union { variants }) => {
                    variants.iter().map(|v| v.name.clone()).collect()
                }
                _ => Vec::new(),
            };
            let suggestion = dz_resolve::closest(&name.name, names.iter().map(String::as_str));
            let mut d = dz_diag::Diagnostic::error(
                codes::NO_SUCH_FIELD,
                format!("`{owner}` has no variant `{}`", name.name),
            )
            .at(self.cx.file, name.span, "unknown variant");
            if let Some(s) = suggestion {
                d = d.with_help(format!("did you mean `{s}`?"));
            }
            self.cx.diags.push(d);
        }
        // A variant with a payload is written `.Circle{ radius: 1 }`; the payload is the
        // variant's own field list, which the union's signature carries (spec §7.5).
        match found {
            Some(index) if is_enum => {
                for f in init.unwrap_or(&[]) {
                    self.expr(&f.value, None);
                }
                hir::ExprKind::EnumVariant { owner: sym, index }
            }
            Some(index) => {
                let args = match want {
                    Ty::Named { args, .. } => args.clone(),
                    _ => Vec::new(),
                };
                self.union_variant_literal(sym, &args, index, init.unwrap_or(&[]), span)
            }
            None => {
                for f in init.unwrap_or(&[]) {
                    self.expr(&f.value, None);
                }
                hir::ExprKind::Error
            }
        }
    }

    fn switch(&mut self, s: &SwitchExpr, e: &Expr, expected: Option<&Ty>) -> Checked {
        let wanted = self.value_wanted(expected);
        if wanted {
            self.pending_nones.push(Vec::new());
        }
        let subject = self.expr(&s.subject, None);
        let peeled = subject.ty.peeled().clone();
        let sym = peeled.named();

        // An enum or union is matched by variant. Anything else that `==` is exact on — an
        // integer, a `char`, a `bool`, a `str` — is matched against constants (spec §10.3).
        let mut scalar: Option<Ty> = None;
        let variants: Vec<crate::VariantSig> = match sym.and_then(|id| self.cx.table.get(id)) {
            Some(ItemSig::Enum { variants, .. }) | Some(ItemSig::Union { variants }) => {
                variants.clone()
            }
            _ => {
                match peeled.defaulted() {
                    t @ (Ty::Int(_) | Ty::Char | Ty::Bool | Ty::Str) => scalar = Some(t),
                    _ if peeled.is_wildcard() => {}
                    _ => {
                        let n = self.cx.name_of(&peeled);
                        let help = if matches!(peeled, Ty::Float(_) | Ty::FloatLit) {
                            "a float is never matched exactly; compare it with `<` and `>` \
                             in an `if` instead (spec §10.3)"
                        } else {
                            "`switch` matches the variants of an enum or a tagged union, or \
                             an integer, `char`, `bool` or `str` against constants (spec §10.3)"
                        };
                        self.cx.error_help(
                            codes::TYPE_MISMATCH,
                            s.subject.span,
                            format!("cannot switch on `{n}`"),
                            "not something `switch` can match",
                            help,
                        );
                    }
                }
                Vec::new()
            }
        };
        let is_union =
            matches!(sym.and_then(|id| self.cx.table.get(id)), Some(ItemSig::Union { .. }));

        let mut covered: Vec<String> = Vec::new();
        // Constant labels already seen, by value, for DZ0548.
        let mut seen: Vec<String> = Vec::new();
        let mut result: Option<Ty> = None;
        let mut hir_arms: Vec<hir::Arm> = Vec::new();

        for arm in &s.arms {
            self.push();
            let mut indices = Vec::new();
            let mut values = Vec::new();
            let mut binding = None;
            for pat in &arm.patterns {
                if let Some(ty) = &scalar {
                    if let Some(v) = self.case_value(pat, ty, &mut seen) {
                        values.push(v);
                    }
                    continue;
                }
                let pat = match pat {
                    CasePat::Variant(v) => v,
                    CasePat::Value(value) => {
                        // `case Idle` split over lines lands here as a one-name path.
                        if let ExprKind::Path(p) = &value.kind {
                            if p.segments.len() == 1 && !variants.is_empty() {
                                let name = &p.segments[0].name;
                                if let Some(i) = variants.iter().position(|v| v.name == name.name) {
                                    self.note_duplicate_variant(
                                        &covered,
                                        &variants[i].name,
                                        name.span,
                                    );
                                    covered.push(variants[i].name.clone());
                                    indices.push(i);
                                    continue;
                                }
                            }
                        }
                        if !variants.is_empty() {
                            let owner = sym
                                .map(|id| self.cx.res.symbol(id).name.clone())
                                .unwrap_or_default();
                            self.cx.error_help(
                                codes::BAD_CASE_LABEL,
                                value.span,
                                format!("a `case` on `{owner}` names a variant"),
                                "not a variant name",
                                "an enum or union is matched by variant: `case Idle`, \
                                 `case Walking(w)` (spec §10.3)",
                            );
                        }
                        continue;
                    }
                };
                let known = variants.iter().find(|v| v.name == pat.name.name).cloned();
                if let Some(v) = &known {
                    self.note_duplicate_variant(&covered, &v.name, pat.name.span);
                }
                match &known {
                    Some(v) => {
                        covered.push(v.name.clone());
                        if let Some(i) = variants.iter().position(|x| x.name == v.name) {
                            indices.push(i);
                        }
                        if let Some(b) = &pat.binding {
                            // The payload's type is the variant's field list, not the
                            // union — without that, `c.radius` had nothing to resolve
                            // against. `c` copies it, `&c` and `&mut c` refer to it, as a
                            // `for` binding does (spec §10.3, §10.4).
                            let index = variants.iter().position(|x| x.name == v.name);
                            let args = match &peeled {
                                Ty::Named { args, .. } => args.clone(),
                                _ => Vec::new(),
                            };
                            // A variant carrying a struct binds that struct, methods and all
                            // (spec §7.5, 18.45).
                            let payload = match (is_union, sym, index) {
                                (true, Some(_), Some(i)) if variants[i].carries.is_some() => {
                                    variants[i].carries.map_or(Ty::Error, Ty::plain)
                                }
                                (true, Some(owner), Some(i)) => Ty::Variant(owner, i, args),
                                _ => Ty::Error,
                            };
                            let ty = self.case_binding_type(pat, payload, &s.subject, &subject.ty);
                            binding =
                                Some(self.bind(&b.name, ty, false, LocalKind::Binding, b.span));
                        }
                    }
                    None => {
                        if !variants.is_empty() {
                            let owner = sym
                                .map(|id| self.cx.res.symbol(id).name.clone())
                                .unwrap_or_default();
                            let names: Vec<String> =
                                variants.iter().map(|v| v.name.clone()).collect();
                            let suggestion = dz_resolve::closest(
                                &pat.name.name,
                                names.iter().map(String::as_str),
                            );
                            let mut d = dz_diag::Diagnostic::error(
                                codes::NO_SUCH_FIELD,
                                format!("`{owner}` has no variant `{}`", pat.name.name),
                            )
                            .at(
                                self.cx.file,
                                pat.name.span,
                                "unknown variant",
                            );
                            if let Some(x) = suggestion {
                                d = d.with_help(format!("did you mean `{x}`?"));
                            }
                            self.cx.diags.push(d);
                        }
                        if let Some(b) = &pat.binding {
                            binding = Some(self.bind(
                                &b.name,
                                Ty::Error,
                                true,
                                LocalKind::Binding,
                                b.span,
                            ));
                        }
                    }
                }
            }
            let body = self.block_valued(&arm.body, expected, wanted);
            result = Some(match result {
                Some(prev) => join(&prev, &body.ty).unwrap_or(Ty::Void),
                None => body.ty.clone(),
            });
            hir_arms.push(hir::Arm {
                span: arm.span,
                variants: indices,
                values,
                binding,
                body: body.id,
            });
            self.pop();
        }

        let mut default = None;
        if let Some(d) = &s.default {
            let c = self.block_valued(d, expected, wanted);
            default = Some(c.id);
            result = Some(match result {
                Some(prev) => join(&prev, &c.ty).unwrap_or(Ty::Void),
                None => c.ty,
            });
        } else if !variants.is_empty() {
            let uncovered: Vec<&_> =
                variants.iter().filter(|v| !covered.contains(&v.name)).collect();
            if !uncovered.is_empty() {
                let owner = sym.map(|id| self.cx.res.symbol(id).name.clone()).unwrap_or_default();
                let list = uncovered
                    .iter()
                    .map(|v| format!("`{}`", v.name))
                    .collect::<Vec<_>>()
                    .join(", ");
                // The arms this wants are already known exactly, so the diagnostic carries
                // them as an edit and not only as a sentence (spec §17.4). A variant that
                // carries a payload needs a binding, and `_` is the one that says the arm
                // does not use it yet — a named binding would come with an unused-variable
                // warning the moment the fix was applied.
                let arms: Vec<String> = uncovered
                    .iter()
                    // A variant has a payload either by naming a struct (`carries`) or by
                    // declaring fields of its own; an enum's variants and a union's empty
                    // ones have neither, and take no binding (spec §7.5, 18.45).
                    .map(|v| {
                        if v.carries.is_some() || !v.fields.is_empty() {
                            format!("case {}(_) {{}}", v.name)
                        } else {
                            format!("case {} {{}}", v.name)
                        }
                    })
                    .collect();
                let fix = self.cx.insert_inside_braces(s.span, &arms);
                let mut d = Diagnostic::error(
                    codes::NON_EXHAUSTIVE_SWITCH,
                    format!("`switch` on `{owner}` is not exhaustive"),
                )
                .at(self.cx.file, s.span, format!("{list} not covered"))
                .with_help("add the missing arms, or a `default` arm (spec §10.3)");
                if let Some(edit) = fix {
                    let what = if arms.len() == 1 { "arm" } else { "arms" };
                    d = d.with_fix(format!("add the missing {what}"), vec![edit]);
                }
                self.cx.diags.push(d);
            }
        }

        let ty = result.unwrap_or(Ty::Void);
        let kind = hir::ExprKind::Switch { subject: subject.id, arms: hir_arms, default };
        let c = self.mk(e, ty, kind);
        self.settle_pending_nones(wanted, c, e.span)
    }

    /// Give the `none` arms of the `if` or `switch` just built the type its arms joined to
    /// (spec §9.1, 18.53). When they are all `none`, the enclosing arm may still decide, so
    /// they wait there; with nothing enclosing, the function's `T?` return type decides, as
    /// it always has for `none`, and otherwise there is nothing to go by.
    fn settle_pending_nones(&mut self, wanted: bool, c: Checked, span: Span) -> Checked {
        if !wanted {
            return c;
        }
        let Some(scope) = self.pending_nones.pop() else { return c };
        if scope.is_empty() {
            return c;
        }
        if !ty::is_pending_none(&c.ty) && (matches!(c.ty, Ty::Optional(_)) || c.ty.is_error()) {
            for (id, _) in scope {
                self.hir.set_expr_type(id, c.ty.clone());
            }
            return c;
        }
        if ty::is_pending_none(&c.ty) {
            if let Some(outer) = self.pending_nones.last_mut() {
                outer.extend(scope);
                outer.push((c.id, span));
                return c;
            }
        }
        let fallback = match &self.ret {
            Ty::Optional(inner) => Ty::Optional(inner.clone()),
            _ => Ty::Error,
        };
        for (id, at) in scope {
            // A block waiting only because its value is a `none` is not a second mistake.
            if fallback.is_error() && matches!(self.hir.kind(id), hir::ExprKind::NoneOpt) {
                self.cx.error_help(
                    codes::NO_CONTEXT_FOR_LITERAL,
                    at,
                    "`none` needs an optional type here",
                    "nothing here expects a `T?`",
                    "`none` is the empty case of `T?`; give the binding a type, `let x: i32? = …`, \
                     or make another arm a value (spec §9.1)",
                );
            }
            self.hir.set_expr_type(id, fallback.clone());
        }
        if ty::is_pending_none(&c.ty) {
            self.hir.set_expr_type(c.id, fallback.clone());
            return Checked { ty: fallback, id: c.id };
        }
        c
    }

    /// The `Nullable` implementation of `t`, when it is a handle — a struct that `null`,
    /// `== null`, `or` and `if let` treat as a pointer (spec §14.5, 18.62).
    fn nullable(&self, t: &Ty, span: Span) -> Option<TraitRef> {
        if !matches!(t, Ty::Named { .. }) {
            return None;
        }
        let trait_sym = self.cx.prelude_trait("Nullable")?;
        let r = TraitRef { trait_sym, args: Vec::new(), span };
        self.satisfies(t, &r).then_some(r)
    }

    /// `recv.name()` through the handle's `Nullable` implementation, typed by it.
    fn nullable_call(
        &mut self,
        r: &TraitRef,
        recv: hir::ExprId,
        recv_ty: &Ty,
        name: &str,
        span: Span,
    ) -> Checked {
        let ret = match self.cx.table.trait_method(r, recv_ty, name) {
            Some(sig) => self.subst(&sig.ret),
            None => Ty::Error,
        };
        let callee = hir::Callee::Method {
            recv,
            owner: r.trait_sym,
            name: name.to_owned(),
            method_args: Vec::new(),
        };
        self.hir.expr(span, ret, hir::ExprKind::Call { callee, args: Vec::new() })
    }

    /// A handle reached by `or` or `if let`: its guard, held in a hidden local until the
    /// enclosing block ends, and the target read out of that guard as a pointer — as
    /// writable as `writable` says (spec §14.5, 18.62). What the `or` or `if let` then does
    /// with the pointer is exactly what it does with a `^T`.
    ///
    /// `subject` is the handle, or a reference to one. `None` when it is not a handle.
    fn guarded_handle(&mut self, subject: &Checked, ast: &Expr) -> Option<(Checked, Ty, bool)> {
        let (handle_ty, via_ref) = match &subject.ty {
            Ty::Ref { mutable, inner } => ((**inner).clone(), Some(*mutable)),
            t => (t.clone(), None),
        };
        let r = self.nullable(&handle_ty, ast.span)?;
        let span = ast.span;
        let guard = self.nullable_call(&r, subject.id, &handle_ty, "guard", span);
        let guard_ty = guard.ty.clone();
        let target = match self.nullable(&guard_ty, span) {
            Some(gr) => {
                let local = self.hir.local("_guard", guard_ty.clone(), false, span, LocalKind::Let);
                let read = self.hir.expr(span, guard_ty.clone(), hir::ExprKind::Local(local));
                let ptr = self.nullable_call(&gr, read.id, &guard_ty, "ptr", span);
                Some((local, ptr))
            }
            None => None,
        };
        let Some((local, ptr)) = target else {
            let (h, g) = (self.cx.name_of(&handle_ty), self.cx.name_of(&guard_ty));
            self.cx.error_help(
                codes::NOT_FALLIBLE,
                span,
                format!("`{h}`'s guard `{g}` is not `Nullable`"),
                "its target cannot be read through the guard",
                "a `Nullable` implementation's `Guard` must itself be `Nullable` (spec §14.5)",
            );
            return Some((Checked { ty: Ty::Error, id: subject.id }, Ty::Error, true));
        };
        let target_ty = match &ptr.ty {
            Ty::Ptr { inner, .. } | Ty::OwnPtr { inner, .. } => (**inner).clone(),
            _ => Ty::Error,
        };
        // Writable when the handle is: through a `&mut`, or a place that may be written.
        // What the handle points at is not the handle, but a read-only handle handing out
        // a writable target would make `&self` methods able to change what they share.
        let writable = match via_ref {
            Some(m) => m,
            None => self.place_mutability(ast).unwrap_or(true),
        };
        let ty = Ty::ptr(writable, target_ty.clone());
        let c = self.hir.expr(
            span,
            ty,
            hir::ExprKind::Guarded { guard: local, init: guard.id, value: ptr.id },
        );
        Some((c, target_ty, writable))
    }

    /// The type of an `if let` binding, which holds what is inside the subject as a `for`
    /// binding holds an element (spec §9.4, 18.61): `v` a copy that cannot be changed (`mut v`
    /// one that can), `&v` a read-only reference, `&mut v` a writable one.
    ///
    /// `writable` is what the subject's own type says — a `^mut T` or a `&mut T?` — and
    /// `None` when it is a value, whose place decides. `borrowed` is set when the payload
    /// lives somewhere the `if let` does not own, so `v` can only copy it: an optional held
    /// in a local is taken out of a temporary and may be moved.
    fn if_let_binding(
        &mut self,
        b: &dz_ast::IfBinding,
        subject: &Expr,
        payload: &Ty,
        writable: Option<bool>,
        borrowed: bool,
    ) -> Ty {
        let n = &b.name.name;
        match b.mode {
            BindMode::Value => {
                if borrowed && !payload.is_wildcard() && !self.cx.table.copyable(payload) {
                    let t = self.cx.name_of(payload);
                    self.cx.error_help(
                        codes::IMPLICIT_COPY,
                        b.name.span,
                        format!("`{n}` would be a copy of a `{t}`, which cannot be copied"),
                        "a copy of the value",
                        format!("refer to it with `if let &{n}` or `&mut {n}` (spec §6.3, §9.4)"),
                    );
                    return Ty::Error;
                }
                payload.clone()
            }
            BindMode::Ref => Ty::reference(false, payload.clone()),
            BindMode::RefMut => {
                let ok = match writable {
                    Some(w) => w,
                    None if payload.is_wildcard() => true,
                    // A temporary optional is the `if let`'s own, so it may be written.
                    None => self.place_mutability(subject).unwrap_or(true),
                };
                if ok {
                    self.require_unsafe_static_write(subject);
                } else if !payload.is_wildcard() {
                    self.cx.error_help(
                        codes::NOT_MUTABLE,
                        b.name.span,
                        format!("cannot refer to this value with `&mut {n}`"),
                        "it is read-only here",
                        "take it out of a `let mut`, a `&mut` or a `^mut`, or read it with \
                         `&` (spec §9.4)",
                    );
                }
                // Writable as asked either way, so the writes in the block are not each
                // reported again: the mistake is the binding, once (invariant 6).
                Ty::reference(true, payload.clone())
            }
        }
    }

    /// The type of a `case` payload binding, which follows the `for` rules exactly (spec
    /// §10.3, 18.43): `case Circle(c)` is a copy that cannot be changed, `&c` a read-only
    /// reference, and `&mut c` a writable one — which needs the union itself to be writable.
    fn case_binding_type(
        &mut self,
        pat: &VariantPat,
        payload: Ty,
        subject: &Expr,
        subject_ty: &Ty,
    ) -> Ty {
        match pat.mode {
            BindMode::Value => {
                if let Ty::Variant(owner, index, args) = &payload {
                    let fields = match self.cx.table.get(*owner) {
                        Some(ItemSig::Union { variants }) => {
                            variants.get(*index).map(|v| v.fields.clone()).unwrap_or_default()
                        }
                        _ => Vec::new(),
                    };
                    let types = self.cx.table.field_types(*owner, args, &fields);
                    if let Some(t) =
                        types.iter().find(|t| !t.is_wildcard() && !self.cx.table.copyable(t))
                    {
                        let t = self.cx.name_of(t);
                        let n = pat.binding.as_ref().map_or("c".to_owned(), |b| b.name.clone());
                        self.cx.error_help(
                            codes::IMPLICIT_COPY,
                            pat.span,
                            format!("`{n}` would be a copy of a payload holding a `{t}`, which cannot be copied"),
                            "a copy of the payload",
                            format!(
                                "refer to the payload with `{}(&{n})` (spec §6.3, §10.3)",
                                pat.name.name
                            ),
                        );
                    }
                } else if payload.named().is_some() && !self.cx.table.copyable(&payload) {
                    // A carried struct that cannot be copied (spec §7.5, 18.45).
                    let t = self.cx.name_of(&payload);
                    let n = pat.binding.as_ref().map_or("c".to_owned(), |b| b.name.clone());
                    self.cx.error_help(
                        codes::IMPLICIT_COPY,
                        pat.span,
                        format!("`{n}` would be a copy of a `{t}`, which cannot be copied"),
                        "a copy of the payload",
                        format!(
                            "refer to the payload with `{}(&{n})` (spec §6.3, §10.3)",
                            pat.name.name
                        ),
                    );
                }
                payload
            }
            BindMode::Ref => Ty::reference(false, payload),
            BindMode::RefMut => {
                let ty = subject_ty;
                let writable = match ty {
                    Ty::Ref { mutable, .. }
                    | Ty::Ptr { mutable, .. }
                    | Ty::OwnPtr { mutable, .. } => *mutable,
                    _ if ty.is_wildcard() => true,
                    _ => self.place_mutability(subject) == Some(true),
                };
                if writable {
                    self.require_unsafe_static_write(subject);
                }
                if !writable {
                    self.cx.error_help(
                        codes::NOT_MUTABLE,
                        pat.span,
                        "cannot change the payload of a read-only value",
                        "`&mut` on something that cannot be written",
                        "switch on a `let mut` binding, a `&mut` or a `^mut`, or read the payload \
                         with a plain binding (spec §10.3)",
                    );
                }
                // Writable as asked either way, so the writes in the arm are not each
                // reported again: the mistake is the binding, once (invariant 6).
                Ty::reference(true, payload)
            }
        }
    }

    /// `case A` twice in one `switch`. The second arm could never run.
    fn note_duplicate_variant(&mut self, covered: &[String], name: &str, span: Span) {
        if covered.iter().any(|c| c == name) {
            self.cx.error_help(
                codes::DUPLICATE_CASE,
                span,
                format!("`{name}` is already matched by an earlier `case`"),
                "matched twice",
                "an arm runs only for what no earlier arm matched, so this one never would; \
                 remove it (spec §10.3)",
            );
        }
    }

    /// One label of a `switch` on an integer, `char`, `bool` or `str` (spec §10.3): a
    /// constant of the subject's type. Returns the checked label, or `None` where it was
    /// not one (already reported).
    fn case_value(
        &mut self,
        pat: &CasePat,
        ty: &Ty,
        seen: &mut Vec<String>,
    ) -> Option<hir::ExprId> {
        let synthetic;
        let value: &Expr = match pat {
            CasePat::Value(v) => v,
            CasePat::Variant(v) => {
                if let Some(b) = &v.binding {
                    let n = self.cx.name_of(ty);
                    self.cx.error_help(
                        codes::BAD_CASE_LABEL,
                        v.span,
                        format!("a `case` on `{n}` has no payload to bind"),
                        format!("`{}` binds nothing here", b.name),
                        "only a union variant carries a payload; write the constant alone \
                         (spec §10.3)",
                    );
                    return None;
                }
                // A bare name is a constant here, not a variant.
                synthetic = Expr {
                    span: v.span,
                    kind: ExprKind::Path(Path {
                        span: v.span,
                        segments: vec![PathSegment {
                            span: v.name.span,
                            name: v.name.clone(),
                            generics: Vec::new(),
                        }],
                    }),
                };
                &synthetic
            }
        };
        let checked = self.expr(value, Some(ty));
        // A label of the wrong type is one mistake: reported as a mismatch, and not also
        // as "not a constant" (invariant 6).
        if checked.ty.is_error() || !assignable(&self.subst(ty), &self.subst(&checked.ty)) {
            self.expect(ty, &checked.ty, value.span);
            return None;
        }
        if !self.is_constant(checked.id) {
            self.cx.error_help(
                codes::BAD_CASE_LABEL,
                value.span,
                "a `case` label must be a constant",
                "computed at run time",
                "a label is a literal, a `const`, or arithmetic on those — something known \
                 while compiling. To compare against a value only known at run time, use `if` \
                 (spec §10.3)",
            );
            return None;
        }
        // Integer labels compare by value, so `case 4, 2 + 2` is caught; the rest by what
        // was written, which is exact for literals and misses only two differently named
        // constants that happen to be equal.
        let key = match self.cx.const_int(value) {
            Some(n) => Some(format!("int {n}")),
            None => match self.hir.kind(checked.id) {
                hir::ExprKind::Char(c) => Some(format!("char {c}")),
                hir::ExprKind::Bool(b) => Some(format!("bool {b}")),
                hir::ExprKind::Str(s) => Some(format!("str {s}")),
                hir::ExprKind::Const(sym) => Some(format!("const {sym:?}")),
                _ => None,
            },
        };
        if let Some(key) = key {
            if seen.contains(&key) {
                self.cx.error_help(
                    codes::DUPLICATE_CASE,
                    value.span,
                    "this value is already matched by an earlier `case`",
                    "matched twice",
                    "an arm runs only for what no earlier arm matched, so this label never \
                     would; remove it (spec §10.3)",
                );
            } else {
                seen.push(key);
            }
        }
        Some(checked.id)
    }

    /// Is this checked expression known while compiling: a literal, a `const`, or operators
    /// and casts over those?
    fn is_constant(&self, id: hir::ExprId) -> bool {
        match self.hir.kind(id) {
            hir::ExprKind::Int(_)
            | hir::ExprKind::Float(_)
            | hir::ExprKind::Bool(_)
            | hir::ExprKind::Char(_)
            | hir::ExprKind::Str(_)
            | hir::ExprKind::Const(_) => true,
            hir::ExprKind::Unary { operand, .. } | hir::ExprKind::Cast { operand } => {
                self.is_constant(*operand)
            }
            hir::ExprKind::Binary { lhs, rhs, .. } => {
                self.is_constant(*lhs) && self.is_constant(*rhs)
            }
            _ => false,
        }
    }

    /// What a `static` may start as: a constant, `null`, `none`, or a struct, array, tuple or
    /// variant literal of those (spec §7.7). A struct literal's omitted fields take their
    /// declared defaults, which are themselves checked where they are declared.
    fn is_static_initialiser(&self, id: hir::ExprId) -> bool {
        if self.is_constant(id) {
            return true;
        }
        match self.hir.kind(id) {
            hir::ExprKind::Null | hir::ExprKind::NoneOpt | hir::ExprKind::EnumVariant { .. } => {
                true
            }
            hir::ExprKind::StructLit { fields, .. } => {
                fields.iter().flatten().all(|f| self.is_static_initialiser(*f))
            }
            hir::ExprKind::UnionVariant { payload, .. } => {
                payload.iter().flatten().all(|f| self.is_static_initialiser(*f))
            }
            hir::ExprKind::ArrayLit(parts) | hir::ExprKind::Tuple(parts) => {
                parts.iter().all(|p| self.is_static_initialiser(*p))
            }
            _ => false,
        }
    }

    /// `xs[a..b]` (spec §5.3). On an array or a slice both ends are checked when it runs;
    /// on a pointer nothing is, so it is `unsafe` and needs its end written. The slice is as
    /// writable as what it views: a `[]mut T`, a `^mut T`, or an array that may be written.
    fn slice_range(
        &mut self,
        e: &Expr,
        base: &Expr,
        start: Option<&Expr>,
        end: Option<&Expr>,
    ) -> Checked {
        let b = self.expr(base, None);
        let usize_ty = Ty::Int(IntTy::Usize);
        let mut ends = Vec::new();
        for x in [start, end].into_iter().flatten() {
            let c = self.expr(x, Some(&usize_ty));
            self.expect(&usize_ty, &c.ty, x.span);
            ends.push(c.id);
        }
        let start_id = start.map(|_| ends[0]);
        let end_id = end.map(|_| ends[ends.len() - 1]);
        let (elem, mutable) = match b.ty.peeled_refs().clone() {
            Ty::Ptr { inner, mutable } | Ty::OwnPtr { inner, mutable }
                if !matches!(*inner, Ty::Array { .. } | Ty::Slice { .. }) =>
            {
                self.require_unsafe(e.span, "slicing a `^T`");
                if end.is_none() {
                    self.cx.error_help(
                        codes::TYPE_MISMATCH,
                        e.span,
                        "a slice of a pointer needs its end",
                        "no end",
                        "a pointer carries no length, so write where the slice stops: \
                         `p[0..n]` (spec §5.3)",
                    );
                }
                ((*inner).clone(), mutable)
            }
            Ty::Slice { elem, mutable } => ((*elem).clone(), mutable),
            Ty::Array { elem, .. } => {
                let writable = match &b.ty {
                    Ty::Ref { mutable, .. }
                    | Ty::Ptr { mutable, .. }
                    | Ty::OwnPtr { mutable, .. } => *mutable,
                    _ => self.place_mutability(base) == Some(true),
                };
                ((*elem).clone(), writable)
            }
            other => {
                if !other.is_wildcard() {
                    let n = self.cx.name_of(&b.ty);
                    self.cx.error(
                        codes::NOT_INDEXABLE,
                        base.span,
                        format!("`{n}` cannot be sliced"),
                        "not an array, a slice or a pointer",
                    );
                }
                return self.mk(e, Ty::Error, hir::ExprKind::Error);
            }
        };
        let ty = Ty::Slice { mutable, elem: Rc::new(elem) };
        self.mk(e, ty, hir::ExprKind::SliceRange { base: b.id, start: start_id, end: end_id })
    }

    /// What a constant declared in a type may be: anything a `static` may start as, or the
    /// address of a static, bare or in a block (spec §7.3).
    fn is_assoc_const_value(&self, id: hir::ExprId) -> bool {
        if self.is_static_initialiser(id) {
            return true;
        }
        match self.hir.kind(id) {
            hir::ExprKind::Ref { place, .. } => {
                let mut at = *place;
                loop {
                    match self.hir.kind(at) {
                        hir::ExprKind::Static(_) => return true,
                        hir::ExprKind::Field { base, .. } => at = *base,
                        _ => return false,
                    }
                }
            }
            hir::ExprKind::Block(b) => match self.hir.block_value(*b) {
                Some(v) => self.is_assoc_const_value(v),
                None => false,
            },
            _ => false,
        }
    }

    /// The `static` a place is rooted in, when writing the place writes the static itself:
    /// `HEAP`, `HEAP.used`, `TABLE[i]` — but not `HEAP.ptr^`, which writes through a
    /// pointer the static merely holds (spec §7.7).
    fn static_root(&mut self, e: &Expr) -> Option<SymbolId> {
        match &e.kind {
            ExprKind::Path(p) => {
                if p.segments.len() == 1 && self.local(&p.segments[0].name.name).is_some() {
                    return None;
                }
                let sym = self.path_symbol(p)?;
                (self.cx.res.symbol(sym).kind == SymbolKind::Static).then_some(sym)
            }
            ExprKind::Field { base, .. } | ExprKind::Index { base, .. } => {
                match self.expr_quiet(base) {
                    Ty::Ptr { .. } | Ty::OwnPtr { .. } | Ty::Ref { .. } | Ty::Slice { .. } => None,
                    _ => self.static_root(base),
                }
            }
            _ => None,
        }
    }

    /// Changing a `static` is `unsafe`: it is one variable shared by all the program, or by
    /// all of a thread, and nothing checks who else is using it (spec §7.7).
    fn require_unsafe_static_write(&mut self, e: &Expr) {
        let Some(sym) = self.static_root(e) else { return };
        if self.unsafe_depth > 0 {
            return;
        }
        let nested = |a: &Span, b: &Span| a.start <= b.start && b.end <= a.end;
        if self.unsafe_reported.iter().any(|s| nested(s, &e.span) || nested(&e.span, s)) {
            return;
        }
        self.unsafe_reported.push(e.span);
        let name = self.cx.res.symbol(sym).name.clone();
        self.cx.error_help(
            codes::UNSAFE_OPERATION,
            e.span,
            format!("changing the static `{name}` is unsafe"),
            "outside an `unsafe` block",
            "wrap it in `unsafe { … }`: a `static` is shared by everything that names it, and \
             nothing checks what else is reading or holding it (spec §7.7)",
        );
    }

    // --- the `or` clause (spec §9.4) --------------------------------------

    /// `e or clause`.
    ///
    /// Three different things can fail, and this is the one eliminator for all of them: a
    /// fallible call carries an `Error`, a `T?` carries nothing, and a `^T` carries
    /// nothing. The single rule the clause must satisfy is that it produces the success
    /// type or diverges.
    fn or_expr(
        &mut self,
        operand: &Expr,
        clause: &OrClause,
        e: &Expr,
        _expected: Option<&Ty>,
    ) -> Checked {
        // The clause's own position is what matters; the operand is a subexpression.
        let stmt_position = std::mem::replace(&mut self.stmt_position, false);
        let subject = self.expr(operand, None);
        // An `Rc`, `Weak` or `Box` is reached as a `^T` is, through a guard (spec §14.5).
        let subject = match self.guarded_handle(&subject, operand) {
            Some((guarded, _, _)) => guarded,
            None => subject,
        };
        // `b or 5` with `b: &u32?` — what `^u32? or return` gives — reads the optional the
        // reference refers to, as a reference reads as its value wherever one is wanted
        // (18.35). Only a copy: taking a move-only payload out would move out of a borrow.
        let subject_ty = match &subject.ty {
            Ty::Ref { inner, .. } if matches!(**inner, Ty::Optional(_)) => {
                if !self.cx.table.copyable(inner) {
                    let n = self.cx.name_of(inner);
                    self.cx.error_help(
                        codes::MOVE_OUT_OF_BORROW,
                        operand.span,
                        format!("`or` would move the value out of this `&{n}`"),
                        "a reference to an optional that cannot be copied",
                        "refer to the value instead: `if let &v = …` (spec §9.4)",
                    );
                }
                (**inner).clone()
            }
            t => t.clone(),
        };
        let (success, payload) = match &subject_ty {
            // A fallible call is the only shape that carries an `Error` to bind.
            Ty::Fallible(inner) => {
                ((**inner).clone(), Some(Ty::Builtin(crate::ty::BuiltinTy::Error)))
            }
            Ty::Optional(inner) => ((**inner).clone(), None),
            // `^T or …` is the checked pointer-to-reference conversion of §5.2: the
            // failure case is null, and the success case is the reference it becomes — as
            // writable as the pointer was, so `^mut T` gives a `&mut T` and `^T` a `&T`
            // (§18.37). Safe code: the null check is the one thing that can be checked, and
            // whether the pointee is still alive is the programmer's (§1.1).
            Ty::Ptr { mutable, inner } | Ty::OwnPtr { mutable, inner } => {
                (Ty::Ref { mutable: *mutable, inner: inner.clone() }, None)
            }
            t if t.is_wildcard() => (Ty::Error, None),
            t => {
                let n = self.cx.name_of(t);
                self.cx.error_help(
                    codes::NOT_FALLIBLE,
                    operand.span,
                    format!("`or` needs something that can fail, found `{n}`"),
                    "this cannot fail",
                    "`or` applies to a fallible call, a `T?`, or a `^T` (spec §9.4)",
                );
                (Ty::Error, None)
            }
        };

        // Bare `or return` means propagate, and is legal only where the enclosing
        // signature can carry the failure (spec §9.5). Reserving it for propagation is
        // what keeps "I passed this up" and "I threw this away" from sharing a spelling.
        if let OrClause::Value(v) = clause {
            if matches!(v.kind, ExprKind::Return(None)) {
                let carries = match &subject.ty {
                    Ty::Fallible(_) => self.fallible,
                    _ => matches!(self.ret, Ty::Optional(_) | Ty::Void | Ty::Error),
                };
                if self.refuse_return_in_base_init(v.span) {
                    // Said once, as the base initialiser's mistake (invariant 6).
                } else if !carries {
                    let ret = self.cx.name_of(&self.ret.clone());
                    let help = if matches!(subject.ty, Ty::Fallible(_)) {
                        "mark this function `!->`, or handle the failure here with a fallback or a block (spec §9.5)"
                            .to_owned()
                    } else {
                        format!(
                            "this function returns `{ret}`, so a bare `return` has no value to give — supply a fallback instead (spec §9.5)"
                        )
                    };
                    self.cx.error_help(
                        codes::TRY_OUTSIDE_FALLIBLE,
                        v.span,
                        "the failure has nowhere to go",
                        "`or return` propagates, and this signature cannot carry it",
                        help,
                    );
                }
                // Build the node without re-reporting: the propagation rule above has
                // already said everything there is to say about this `return`.
                let c = self.hir.expr(v.span, Ty::Never, hir::ExprKind::Return(None)).id;
                return self.mk(
                    e,
                    success,
                    hir::ExprKind::Or { operand: subject.id, clause: hir::OrClause::Value(c) },
                );
            }
        }

        let hir_clause = match clause {
            OrClause::Value(v) => {
                // `or return`, `or break`, `or continue` and `or panic(…)` arrive here as
                // the diverging expressions they are, and `Never` satisfies anything — so
                // divergence needs no special case, only the type rule.
                let c = self.expr(v, Some(&success));
                if !matches!(c.ty, Ty::Never) {
                    self.expect_or_clause(&success, &c.ty, v.span);
                }
                hir::OrClause::Value(c.id)
            }
            OrClause::Block { binding, body } => {
                self.push();
                let local = match binding {
                    Some(b) => {
                        if payload.is_none() && !subject.ty.is_wildcard() {
                            let n = self.cx.name_of(&subject.ty);
                            self.cx.error_help(
                                codes::OR_CLAUSE_TYPE,
                                b.span,
                                format!("`{n}` has no failure value to bind as `{}`", b.name),
                                "nothing to bind here",
                                "only a fallible call carries an `Error`; write `or { … }` \
                                 with no binding (spec §9.4)",
                            );
                        }
                        let bound = payload.clone().unwrap_or(Ty::Error);
                        Some(self.bind(&b.name, bound, false, LocalKind::Binding, b.span))
                    }
                    None => None,
                };
                let c = self.block(body, Some(&success));
                if !matches!(c.ty, Ty::Never | Ty::Void) {
                    self.expect_or_clause(&success, &c.ty, body.span);
                }
                self.pop();
                hir::OrClause::Block { binding: local, body: c.id }
            }
            // `or ignore` produces nothing, so it is legal only where nothing is wanted.
            OrClause::Ignore(span) => {
                if !stmt_position {
                    self.cx.error_help(
                        codes::IGNORE_PRODUCES_NO_VALUE,
                        *span,
                        "`ignore` produces no value",
                        "nothing to bind here",
                        "`or ignore` is statement position only; to keep a value supply a \
                         fallback instead (spec §9.4)",
                    );
                }
                hir::OrClause::Ignore
            }
            OrClause::Unreachable(_) => hir::OrClause::Unreachable,
        };

        // `or ignore` discards, so the expression yields nothing at all.
        let ty = match clause {
            OrClause::Ignore(_) => Ty::Void,
            _ => success,
        };
        self.mk(e, ty, hir::ExprKind::Or { operand: subject.id, clause: hir_clause })
    }

    /// The clause's own type rule, reported against the clause rather than the operand.
    fn expect_or_clause(&mut self, expected: &Ty, actual: &Ty, span: Span) {
        let expected = self.subst(expected);
        let actual = self.subst(actual);
        if assignable(&expected, &actual) {
            return;
        }
        // A fallible fallback — `a() or b()` where `b` can fail too. Nesting is a real
        // thing to want; it just has to be written out, because an `or` always produces
        // the success type and so can never be chained onto.
        if let Ty::Fallible(_) = &actual {
            self.cx.error_help(
                codes::UNUSED_RESULT,
                span,
                "this fallback can fail too, and its failure is not consumed",
                "this may fail",
                "give it a clause of its own: `a() or { b() or <fallback> }` (spec §9.4)",
            );
            return;
        }
        let (x, a) = (self.cx.name_of(&expected), self.cx.name_of(&actual));
        self.cx.error_help(
            codes::OR_CLAUSE_TYPE,
            span,
            format!("this `or` clause produces `{a}`, but `{x}` is needed"),
            format!("this is `{a}`"),
            "an `or` clause must produce the success type or diverge (spec §9.4)",
        );
    }
}

/// The type a call expression has.
///
/// A `!->` call yields a `Ty::Fallible`, which is not a type anyone can write (spec §9):
/// it exists only between the call and the `or` clause that must consume it in the same
/// statement, and the consumption rule is what stops it going anywhere else.
fn fallible_result(sig: &FnSig, ret: Ty) -> Ty {
    if sig.fallible {
        Ty::Fallible(Rc::new(ret))
    } else {
        ret
    }
}

/// `..xs` as an argument: parsed as a range with no start, and a spread wherever a variadic
/// parameter takes it (spec §7.2).
fn spread(e: &Expr) -> Option<&Expr> {
    match &e.kind {
        ExprKind::Range { start: None, end: Some(inner), inclusive: false } => Some(inner),
        _ => None,
    }
}

/// Does this expression have no type of its own, so that it needs one from context?
///
/// `.Variant`, `none` and an empty array literal are the three. Each already reports
/// `DZ0521` when there is nothing to read a type from, so this only decides which side of a
/// comparison to check first.
fn needs_context(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::DotInfer { .. } | ExprKind::None => true,
        ExprKind::ArrayLit(elems) => elems.is_empty(),
        _ => false,
    }
}

/// What a pointer or reference points at.
fn pointee(t: &Ty) -> Option<&Ty> {
    match t {
        Ty::Ref { inner, .. } | Ty::Ptr { inner, .. } | Ty::OwnPtr { inner, .. } => Some(inner),
        _ => None,
    }
}

/// Match a declaration's type against a concrete one, solving for generic parameters
/// (spec §8.1).
///
/// Structural and one-pass. Where the two shapes disagree it simply stops looking rather
/// than reporting: the argument is checked against the solved signature afterwards, and
/// that check produces a diagnostic phrased in terms of the types the user wrote. A
/// parameter already solved is not overwritten — the first argument that determines it
/// wins, and a later disagreement surfaces as an ordinary mismatch on that argument.
fn unify(pattern: &Ty, actual: &Ty, out: &mut HashMap<u32, Ty>) {
    match (pattern, actual) {
        (Ty::Param(p), _) => {
            if !actual.is_wildcard() {
                out.entry(p.id).or_insert_with(|| actual.clone());
            }
        }
        (
            Ty::Ptr { inner: a, .. } | Ty::OwnPtr { inner: a, .. },
            Ty::Ptr { inner: b, .. } | Ty::OwnPtr { inner: b, .. },
        ) => unify(a, b, out),
        // `&T` also matches a `&mut U`, since `&mut U` may be passed where `&U` is wanted.
        (Ty::Ref { inner: a, .. }, Ty::Ref { inner: b, .. }) => unify(a, b, out),
        (Ty::Optional(a), Ty::Optional(b)) => unify(a, b, out),
        (Ty::Fallible(a), Ty::Fallible(b)) => unify(a, b, out),
        (Ty::Range(a), Ty::Range(b)) => unify(a, b, out),
        // A slice parameter also matches a fixed array, which coerces to one (§5.3).
        (Ty::Slice { elem: a, .. }, Ty::Slice { elem: b, .. } | Ty::Array { elem: b, .. }) => {
            unify(a, b, out)
        }
        (Ty::Array { elem: a, .. }, Ty::Array { elem: b, .. }) => unify(a, b, out),
        (Ty::Tuple(a), Ty::Tuple(b)) if a.len() == b.len() => {
            for (x, y) in a.iter().zip(b) {
                unify(x, y, out);
            }
        }
        (Ty::Named { sym: a, args: xs }, Ty::Named { sym: b, args: ys })
            if a == b && xs.len() == ys.len() =>
        {
            for (x, y) in xs.iter().zip(ys) {
                unify(x, y, out);
            }
        }
        (Ty::Fn { params: ps, ret: r1 }, Ty::Fn { params: qs, ret: r2 })
            if ps.len() == qs.len() =>
        {
            for (x, y) in ps.iter().zip(qs) {
                unify(x, y, out);
            }
            unify(r1, r2, out);
        }
        _ => {}
    }
}

/// A trait method's signature as seen through one implementer (spec §7.6).
///
/// A trait writes `Self` for "the implementing type", so reading `Eq::eq` off a `T: Eq`
/// means `fn eq(&self, rhs: &T) -> bool`. Nothing else in the signature changes.
fn instantiate_self(sig: &FnSig, self_ty: &Ty) -> FnSig {
    let empty = HashMap::new();
    let sub = |t: &Ty| ty::substitute_with(t, &empty, Some(self_ty));
    FnSig {
        params: sig
            .params
            .iter()
            .map(|p| crate::ParamSig {
                name: p.name.clone(),
                ty: sub(&p.ty),
                has_default: p.has_default,
                caller_default: p.caller_default.clone(),
                c_cast: p.c_cast.clone(),
            })
            .collect(),
        ret: sub(&sig.ret),
        ..sig.clone()
    }
}

pub(crate) fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        word.to_owned()
    } else {
        format!("{word}s")
    }
}

/// Return types of the builtin calls (spec §5.5, §9.1, §12.9, §13.1).
/// What the runtime shim can write today (spec §12.9). A user type's own `Format`
/// implementation needs `Formatter` to have somewhere to write, which is the stdlib's
/// `String` (§19 M12), so structs, enums and containers are refused rather than miswritten.
fn formattable(t: &Ty) -> bool {
    match t {
        Ty::Int(_) | Ty::Float(_) | Ty::Bool | Ty::Char | Ty::Str | Ty::CStr => true,
        Ty::Ptr { .. } | Ty::OwnPtr { .. } => true,
        Ty::Ref { inner, .. } => formattable(inner),
        _ => false,
    }
}

/// Does a `{:spec}` suit its argument? `Err` carries the label for the diagnostic.
fn spec_fits(spec: &str, t: &Ty) -> Result<(), &'static str> {
    let t = t.peeled();
    match spec {
        "" => Ok(()),
        "x" | "X" if matches!(t, Ty::Int(_)) => Ok(()),
        "x" | "X" => Err("hex needs an integer"),
        s if s.len() > 1 && s.starts_with('.') && s[1..].bytes().all(|b| b.is_ascii_digit()) => {
            if matches!(t, Ty::Float(_)) {
                Ok(())
            } else {
                Err("a precision needs a float")
            }
        }
        _ => Err("not a format spec"),
    }
}

/// `SourceLocation`, read through a reference as `.` would.
pub(crate) fn is_source_location(t: &Ty) -> bool {
    matches!(t.peeled_refs(), Ty::Builtin(dz_hir::ty::BuiltinTy::SourceLocation))
}

/// How many arguments a builtin takes, where codegen depends on the count. `None` for the
/// variadic formatting calls, which §12.9's format-string checking will cover.
fn builtin_arity(name: &str) -> Option<(usize, usize)> {
    Some(match name {
        "alloc" => (2, 2),
        "free" | "destroy" | "move_out" | "sizeof" | "alignof" | "blackbox" => (1, 1),
        // `construct(dst)` default-constructs; `construct(dst, v)` copies or moves in.
        "construct" => (1, 2),
        "memcpy" => (3, 3),
        _ => return None,
    })
}

fn builtin_call_type(name: &str) -> Ty {
    match name {
        "sizeof" | "alignof" => Ty::Int(IntTy::Usize),
        // `alloc` returns `^mut u8` and panics on failure rather than failing (spec §9.8).
        "alloc" => Ty::ptr(true, Ty::Int(IntTy::U8)),
        // `error(...)` is the failure case of whatever `!->` signature encloses it, so its
        // payload is `Never` and unifies with any success type (spec §9.1).
        "error" => Ty::Fallible(Rc::new(Ty::Never)),
        "format" => Ty::Str,
        "typeid_of" => Ty::Int(IntTy::U32),
        // `panic` never returns; the rest produce nothing useful yet.
        "panic" => Ty::Never,
        _ => Ty::Void,
    }
}
