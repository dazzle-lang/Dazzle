//! Type parameter packs at a call (spec §8.7, D-0121): a pack parameter's arguments gathered
//! into the tuple the body sees, and a tuple spread, `..t`, into the arguments it holds.
//!
//! Both are rewritten here, so nothing past checking sees a variable number of arguments.
//! A pack parameter is given one argument, a tuple literal of what the call wrote, as a
//! variadic slice parameter is given one array (§7.2). A spread tuple of known types becomes
//! its elements, `t.0`, `t.1`, …, each an argument of its own. Only a pack's own tuple,
//! `(..T)`, whose length is up to each call, stays a [`hir::ExprKind::Spread`]: one entry,
//! standing in a list exactly where that list has its `..T`, until monomorphization knows
//! the types and the C++ emitter writes one argument per element.

use std::collections::HashMap;

use dz_ast::{Expr, ExprKind};
use dz_diag::codes;
use dz_hir as hir;
use dz_span::Span;

use super::{plural, spread, unify, Body};
use crate::build::Checked;
use crate::ty::{self, Ty};
use crate::{FnSig, ParamDef};

/// The most elements a tuple may have: the C++ runtime spells out each length it holds.
pub(super) const MAX_TUPLE: usize = 16;

/// One argument as the parameters it fills see it.
pub(super) enum Slot<'e> {
    /// An argument as it is written.
    Arg(&'e Expr),
    /// Element `index` of a tuple of known types spread with `..t`.
    Elem { spread: &'e Expr, tuple: &'e Expr, index: usize, ty: Ty },
    /// A pack's tuple spread, `..args` with `args: (..T)`: one entry standing for all of
    /// `T`, whose type is the `..T` it fills.
    Pack { spread: &'e Expr, tuple: &'e Expr, ty: Ty },
    /// An element of a spread already reported, which fits anything, so the count of the
    /// call's arguments is not reported again (invariant 6).
    Broken(&'e Expr),
}

impl Slot<'_> {
    fn span(&self) -> Span {
        match self {
            Slot::Arg(a) | Slot::Broken(a) => a.span,
            Slot::Elem { spread, .. } | Slot::Pack { spread, .. } => spread.span,
        }
    }
}

/// How a slot is held to the type its place in the list wants.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Want {
    /// A named function's parameter: its type, or a conversion to it (§7.2).
    Convert,
    /// A function value's parameter: its type exactly.
    Exact,
    /// A tuple literal's element: a hint for a literal, checked by whoever wanted the tuple.
    Hint,
}

impl Body<'_, '_> {
    /// The tuple `a` spreads, and its parts, when `a` is `..t` and `t` is a tuple.
    fn spread_tuple<'e>(&mut self, a: &'e Expr) -> Option<(&'e Expr, Vec<Ty>)> {
        let inner = spread(a)?;
        match self.expr_quiet(inner) {
            Ty::Tuple(parts) => Some((inner, parts)),
            _ => None,
        }
    }

    /// Whether any of `args` spreads a tuple.
    pub(super) fn has_tuple_spread(&mut self, args: &[Expr]) -> bool {
        args.iter().any(|a| spread(a).is_some())
            && args.iter().any(|a| self.spread_tuple(a).is_some())
    }

    /// The arguments as slots, each tuple spread replaced by its elements, or by one entry
    /// for a pack's tuple. A spread that cannot be one stands for nothing, and is reported
    /// when `report` is set: inference reads the arguments before the call checks them.
    pub(super) fn slots<'e>(&mut self, args: &'e [Expr], report: bool) -> Vec<Slot<'e>> {
        let mut out = Vec::with_capacity(args.len());
        for a in args {
            let Some((tuple, parts)) = self.spread_tuple(a) else {
                out.push(Slot::Arg(a));
                continue;
            };
            if !is_place(tuple) {
                if report {
                    self.cx.error_help(
                        codes::BAD_SPREAD,
                        tuple.span,
                        "`..` spreads a tuple that is not a place",
                        "not a local, a parameter or a field",
                        "each element is read from the tuple in turn, so the tuple must be kept \
                         somewhere: bind it first, `let t = …`, and spread `..t` (spec §8.7)",
                    );
                    self.expr(tuple, None);
                }
                out.extend(parts.iter().map(|_| Slot::Broken(a)));
                continue;
            }
            match ty::pack_in(&parts) {
                None => out.extend(parts.into_iter().enumerate().map(|(index, ty)| Slot::Elem {
                    spread: a,
                    tuple,
                    index,
                    ty,
                })),
                Some(_) if parts.len() == 1 => {
                    out.push(Slot::Pack { spread: a, tuple, ty: parts[0].clone() })
                }
                Some((_, p)) => {
                    if report {
                        let p = p.name.clone();
                        self.cx.error_help(
                            codes::BAD_SPREAD,
                            a.span,
                            format!("this tuple holds the pack `..{p}` and more, so it is not spread"),
                            "a pack beside other elements",
                            "a tuple that is a pack alone, `(..T)`, as a pack parameter is, may be \
                             spread; one with more in it cannot be yet (spec §8.7, T-0269)",
                        );
                        self.expr(tuple, None);
                    }
                    out.push(Slot::Broken(a));
                }
            }
        }
        out
    }

    /// The type a slot gives the list it stands in.
    fn slot_ty(&mut self, s: &Slot) -> Ty {
        match s {
            Slot::Arg(a) => self.expr_quiet(a).defaulted(),
            Slot::Elem { ty, .. } | Slot::Pack { ty, .. } => ty.clone(),
            Slot::Broken(_) => Ty::Error,
        }
    }

    /// Check one slot against `want` as `how` says, and build its node.
    pub(super) fn slot_value(&mut self, s: &Slot, want: Option<&Ty>, how: Want) -> Checked {
        match (s, want) {
            (Slot::Arg(a), Some(w)) if how == Want::Convert => {
                let id = self.argument(a, w);
                Checked { ty: w.clone(), id }
            }
            (Slot::Arg(a), Some(w)) => {
                let c = self.expr(a, Some(w));
                if how == Want::Exact {
                    self.expect(w, &c.ty, a.span);
                }
                c
            }
            (Slot::Arg(a), None) => self.expr(a, None),
            (Slot::Broken(a), _) => self.hir.expr(a.span, Ty::Error, hir::ExprKind::Error),
            (Slot::Elem { spread, tuple, index, ty }, _) => {
                let base = self.expr(tuple, None);
                if let Some(w) = want.filter(|_| how != Want::Hint) {
                    self.expect(w, ty, spread.span);
                }
                let kind = hir::ExprKind::TupleIndex { base: base.id, index: *index };
                self.hir.expr(spread.span, ty.clone(), kind)
            }
            (Slot::Pack { spread, tuple, ty }, _) => {
                let base = self.expr(tuple, None);
                if let Some(w) = want.filter(|_| how != Want::Hint) {
                    self.expect(w, ty, spread.span);
                }
                self.hir.expr(spread.span, base.ty, hir::ExprKind::Spread(base.id))
            }
        }
    }

    /// Solve a generic's parameters from a call's arguments, where a pack parameter takes the
    /// rest of them and a spread tuple stands for its elements (spec §8.1, §8.7).
    pub(super) fn unify_args(&mut self, sig: &FnSig, args: &[Expr], map: &mut HashMap<u32, Ty>) {
        let split = args
            .iter()
            .position(|a| matches!(a.kind, ExprKind::Named { .. }))
            .unwrap_or(args.len());
        let slots = self.slots(&args[..split], false);
        let pack = sig.pack_param();
        let fixed = pack.unwrap_or(sig.params.len()).min(sig.params.len());
        for (s, p) in slots.iter().zip(&sig.params[..fixed]) {
            // A pack spread into one parameter says nothing of it: the call is refused.
            if !p.ty.has_params() || matches!(s, Slot::Pack { .. }) {
                continue;
            }
            let actual = self.slot_ty(s);
            unify(&p.ty, &actual, map);
        }
        if let Some(at) = pack.filter(|&at| slots.len() >= at) {
            let rest: Vec<Ty> = slots[at..].iter().map(|s| self.slot_ty(s)).collect();
            unify(&sig.params[at].ty, &Ty::Tuple(rest), map);
        }
    }

    /// One type argument as written after a name, where `..T` passes a pack on (spec §8.7).
    pub(super) fn written_type_arg(&mut self, t: &dz_ast::Type, handle: bool) -> Ty {
        match &t.kind {
            dz_ast::TypeKind::Expand(inner) => self.cx.lower_expansion(inner, t.span),
            _ => self.cx.lower_checked_type(t, handle),
        }
    }

    /// Type arguments written out, `spawn<Inbox, Outbox>(…)`: the fixed parameters' by
    /// position, and every one after them the pack's, when the last parameter is one
    /// (spec §8.7). Whether the count is right is the caller's to report, so this answers
    /// it.
    pub(super) fn bind_written(
        &mut self,
        generics: &[ParamDef],
        written: Vec<Ty>,
        map: &mut HashMap<u32, Ty>,
    ) -> bool {
        let pack = generics.last().filter(|d| d.param.pack);
        let fixed = generics.len() - usize::from(pack.is_some());
        let fits = match pack {
            Some(_) => written.len() >= fixed,
            None => written.len() == generics.len(),
        };
        let mut written = written.into_iter();
        for (def, t) in generics[..fixed].iter().zip(written.by_ref()) {
            map.insert(def.param.id, t);
        }
        if let Some(def) = pack {
            map.insert(def.param.id, Ty::Tuple(written.collect()));
        }
        fits
    }

    /// A pack's bounds hold of every type it was given (spec §8.7): each is checked as a
    /// parameter of one type with the same bounds would be.
    pub(super) fn check_pack_bounds(&mut self, def: &ParamDef, chosen: &Ty, span: Span) -> bool {
        let Ty::Tuple(parts) = chosen else { return true };
        // Named as written, so a refusal says `..A`, and `view ..A` where it suggests one.
        let one = ParamDef {
            param: ty::Param {
                pack: false,
                name: format!("..{}", def.param.name).into(),
                ..def.param.clone()
            },
            bounds: def.bounds.clone(),
            default: None,
            span: def.span,
        };
        let mut ok = true;
        // A pack passed on, `..T` among them, meets these bounds when its own do.
        for p in parts {
            ok &= self.check_bounds(&one, p, span);
        }
        if ok && parts.len() > MAX_TUPLE {
            self.too_long(parts.len(), span);
            return false;
        }
        ok
    }

    /// A tuple longer than the runtime holds.
    pub(super) fn too_long(&mut self, n: usize, span: Span) {
        self.cx.error_help(
            codes::TUPLE_TOO_LONG,
            span,
            format!("a tuple of {n} elements is longer than {MAX_TUPLE}"),
            "too many elements",
            format!(
                "a tuple, a pack included, holds at most {MAX_TUPLE} elements for now; group \
                 some into a struct (spec §8.7)"
            ),
        );
    }

    /// A call of a function with a pack parameter, `fn f(a: A, args: ..T)`: the fixed
    /// arguments as usual, then every remaining one, spreads included, gathered into the
    /// tuple the parameter is, and any after it by name (spec §7.2, §8.7).
    pub(super) fn check_pack_args(
        &mut self,
        sig: &FnSig,
        at: usize,
        args: &[Expr],
        span: Span,
        name: &str,
    ) -> Vec<hir::ExprId> {
        let split = args
            .iter()
            .position(|a| matches!(a.kind, ExprKind::Named { .. }))
            .unwrap_or(args.len());
        let (positional, named) = args.split_at(split);
        let slots = self.slots(positional, true);
        let mut ids = Vec::with_capacity(sig.params.len());
        if slots.len() < at {
            self.cx.error(
                codes::WRONG_ARG_COUNT,
                span,
                format!(
                    "`{name}` expects at least {at} {}, found {}",
                    plural(at, "argument"),
                    slots.len()
                ),
                "wrong number of arguments",
            );
            for s in &slots {
                ids.push(self.slot_value(s, None, Want::Convert).id);
            }
            return ids;
        }
        for (s, p) in slots[..at].iter().zip(&sig.params[..at]) {
            let want = self.subst(&p.ty);
            ids.push(self.slot_value(s, Some(&want), Want::Convert).id);
        }
        let rest = &slots[at..];
        let parts = match self.subst(&sig.params[at].ty) {
            Ty::Tuple(parts) => parts,
            _ => Vec::new(),
        };
        // Only type arguments written out can disagree with the arguments; inferred ones
        // were read off them.
        if parts.len() != rest.len() && !parts.iter().any(Ty::is_wildcard) {
            let (want, got) = (parts.len(), rest.len());
            self.cx.error(
                codes::WRONG_ARG_COUNT,
                span,
                format!("`{name}`'s pack takes {want} {}, found {got}", plural(want, "argument")),
                "wrong number of arguments for the pack",
            );
        }
        let mut elems = Vec::with_capacity(rest.len());
        for (i, s) in rest.iter().enumerate() {
            elems.push(self.slot_value(s, parts.get(i), Want::Convert).id);
        }
        let at_span = match (rest.first(), rest.last()) {
            (Some(f), Some(l)) => Span { start: f.span().start, end: l.span().end },
            _ => span,
        };
        ids.push(self.hir.expr(at_span, Ty::Tuple(parts), hir::ExprKind::Tuple(elems)).id);
        self.named_args(sig, at, named, span, name, Some(&mut ids));
        ids
    }

    /// A call of a function of fixed arity with a tuple spread among its arguments: each
    /// element is one argument (spec §8.7). A pack's tuple has no fixed count, so it is
    /// refused here.
    pub(super) fn check_spread_args(
        &mut self,
        sig: &FnSig,
        args: &[Expr],
        span: Span,
        name: &str,
    ) -> Vec<hir::ExprId> {
        let slots = self.slots(args, true);
        if let Some(s) = slots.iter().find(|s| matches!(s, Slot::Pack { .. })) {
            self.cx.error_help(
                codes::BAD_SPREAD,
                s.span(),
                format!(
                    "`{name}` takes a fixed number of arguments, and a pack's is up to each call"
                ),
                "a pack spread into fixed parameters",
                "a pack is passed on whole, to a function that takes one: a pack parameter, \
                 `args: ..T`, or a function value of type `fn(..T)` (spec §8.7)",
            );
            return slots.iter().map(|s| self.slot_value(s, None, Want::Convert).id).collect();
        }
        let required = sig.required();
        let total = sig.params.len();
        if slots.len() < required || slots.len() > total {
            let got = slots.len();
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
        let mut ids = Vec::with_capacity(slots.len());
        for (i, s) in slots.iter().enumerate() {
            let want = sig.params.get(i).map(|p| self.subst(&p.ty));
            ids.push(self.slot_value(s, want.as_ref(), Want::Convert).id);
        }
        ids
    }

    /// A call through a function value: one slot per parameter, a pack's spread standing
    /// where the type has its `..T` (spec §5.4, §8.7).
    pub(super) fn fn_value_args(
        &mut self,
        params: &[Ty],
        args: &[Expr],
        span: Span,
    ) -> Vec<hir::ExprId> {
        let slots = self.slots(args, true);
        if params.len() != slots.len() {
            let (want, got) = (params.len(), slots.len());
            self.cx.error(
                codes::WRONG_ARG_COUNT,
                span,
                format!("expected {want} {}, found {got}", plural(want, "argument")),
                "wrong number of arguments",
            );
        }
        let mut ids = Vec::with_capacity(slots.len());
        for (i, s) in slots.iter().enumerate() {
            ids.push(self.slot_value(s, params.get(i), Want::Exact).id);
        }
        ids
    }

    /// `(..t, 3)`: a tuple literal with a spread among its elements (spec §8.7).
    pub(super) fn tuple_with_spread(
        &mut self,
        e: &Expr,
        elems: &[Expr],
        expected: Option<&Ty>,
    ) -> Checked {
        let slots = self.slots(elems, true);
        let want = match expected.map(|t| self.subst(t)) {
            Some(Ty::Tuple(want)) if want.len() == slots.len() => Some(want),
            _ => None,
        };
        let mut ids = Vec::with_capacity(slots.len());
        let mut tys = Vec::with_capacity(slots.len());
        for (i, s) in slots.iter().enumerate() {
            let w = want.as_ref().map(|w| &w[i]);
            let c = self.slot_value(s, w, Want::Hint);
            tys.push(match s {
                Slot::Pack { ty, .. } => ty.clone(),
                _ => c.ty.defaulted(),
            });
            ids.push(c.id);
        }
        if tys.len() > MAX_TUPLE {
            self.too_long(tys.len(), e.span);
        }
        self.mk(e, Ty::Tuple(tys), hir::ExprKind::Tuple(ids))
    }
}

/// Whether `e` is a place a spread may read once per element: a name, or a field or
/// element of one.
fn is_place(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::Path(p) => p.segments.len() == 1,
        ExprKind::Field { base, generics, .. } => generics.is_empty() && is_place(base),
        _ => false,
    }
}
