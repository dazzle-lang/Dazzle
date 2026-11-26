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
//!
//! `..pattern`, where the pattern is not a place and names a pack's tuple, is the pattern
//! once per element, C++'s `f(g(args)...)`: checked once here with the tuple's name standing
//! for the element, an [`hir::ExprKind::Expand`] until monomorphization copies it.

use std::collections::HashMap;

use dz_ast::{Expr, ExprKind, Ident, Path, PathSegment};
use dz_diag::codes;
use dz_hir::{self as hir, TupleEnd};
use dz_span::Span;

use super::{plural, spread, unify, Body};
use crate::build::Checked;
use crate::ty::{self, Ty};
use crate::{FnSig, ParamDef};
use dz_hir::LocalKind;

/// One argument as the parameters it fills see it.
pub(super) enum Slot<'e> {
    /// An argument as it is written.
    Arg(&'e Expr),
    /// Element `index` of a tuple of known types spread with `..t`.
    Elem { spread: &'e Expr, tuple: &'e Expr, index: usize, ty: Ty },
    /// A run of a tuple holding a pack, counted from either end: the pack's own run, one
    /// entry standing for all of `T` whose type `ty` is the `..T` it fills, or one element
    /// after the pack, whose index from the front depends on the pack's length. `node` is
    /// the type its node has: the tuple `(..T)` for the pack's run, the element otherwise.
    /// The first run of a spread, `lead`, takes the tuple; the others continue from it.
    Run {
        spread: &'e Expr,
        tuple: &'e Expr,
        from: TupleEnd,
        to: TupleEnd,
        ty: Ty,
        node: Ty,
        lead: bool,
    },
    /// `..pattern`: the pattern once for each element of the pack's tuple `name`, which
    /// stands for the element inside it (spec §8.7).
    Expand { spread: &'e Expr, pattern: &'e Expr, name: String, pack: ty::Param },
    /// An element of a spread already reported, which fits anything, so the count of the
    /// call's arguments is not reported again (invariant 6).
    Broken(&'e Expr),
}

impl Slot<'_> {
    fn span(&self) -> Span {
        match self {
            Slot::Arg(a) | Slot::Broken(a) => a.span,
            Slot::Elem { spread, .. } | Slot::Run { spread, .. } | Slot::Expand { spread, .. } => {
                spread.span
            }
        }
    }

    /// Whether this slot stands for a whole pack, whose length is up to each call.
    fn is_pack(&self) -> bool {
        match self {
            Slot::Run { ty, .. } => is_pack_entry(ty),
            Slot::Expand { .. } => true,
            _ => false,
        }
    }
}

/// Whether a list's entry stands for a pack's types rather than one type (spec §8.7).
pub(super) fn is_pack_entry(t: &Ty) -> bool {
    ty::pack_entry(t).is_some()
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

    /// Whether any of `args` spreads a tuple, or repeats a pattern over a pack.
    pub(super) fn has_tuple_spread(&mut self, args: &[Expr]) -> bool {
        args.iter().any(|a| spread(a).is_some())
            && args.iter().any(|a| self.expansion(a).is_some() || self.spread_tuple(a).is_some())
    }

    /// `..pattern`, when `a` is one: not a place, and naming one pack's tuple, which it is
    /// repeated over (spec §8.7). The pattern, the tuple's name, and the pack.
    pub(super) fn expansion<'e>(&self, a: &'e Expr) -> Option<(&'e Expr, String, ty::Param)> {
        let inner = spread(a)?;
        if is_place(inner) {
            return None;
        }
        let mut names = Vec::new();
        names_in_expr(inner, &mut names);
        let mut found: Option<(String, ty::Param)> = None;
        for n in names {
            let Some(local) = self.local(&n) else { continue };
            let Ty::Tuple(parts) = &local.ty else { continue };
            let [Ty::Param(p)] = parts.as_slice() else { continue };
            if !p.pack {
                continue;
            }
            match &found {
                Some((m, _)) if *m != n => return None,
                _ => found = Some((n, p.clone())),
            }
        }
        found.map(|(n, p)| (inner, n, p))
    }

    /// The pattern of an expansion checked with `name` standing for the element: its node
    /// and the local the element is (spec §8.7).
    fn check_pattern(
        &mut self,
        pattern: &Expr,
        name: &str,
        pack: &ty::Param,
        want: Option<&Ty>,
        span: Span,
    ) -> (Checked, hir::LocalId) {
        self.push();
        let elem = self.bind(name, Ty::Param(pack.element()), false, LocalKind::Binding, span);
        let c = match want {
            Some(w) => {
                let id = self.argument(pattern, w);
                Checked { ty: w.clone(), id }
            }
            None => self.expr(pattern, None),
        };
        self.pop();
        (c, elem)
    }

    /// The type a pattern makes for each element, without building anything.
    fn pattern_ty(&mut self, pattern: &Expr, name: &str, pack: &ty::Param) -> Ty {
        let mark = self.hir.mark();
        let before = self.cx.diags.len();
        let (c, _) = self.check_pattern(pattern, name, pack, None, pattern.span);
        self.hir.rollback(mark);
        self.cx.diags.truncate(before);
        c.ty.defaulted()
    }

    /// The expansion's node: its tuple, the element's local, and the pattern checked once.
    /// A pattern that cannot be copied per element is reported here.
    fn expand_node(
        &mut self,
        spread_at: Span,
        pattern: &Expr,
        name: &str,
        pack: &ty::Param,
        want: Option<&Ty>,
    ) -> Checked {
        let seg = PathSegment {
            span: pattern.span,
            name: Ident { name: name.to_owned(), span: pattern.span },
            generics: Vec::new(),
        };
        let path = Path { span: pattern.span, segments: vec![seg] };
        let over_expr = Expr { span: pattern.span, kind: ExprKind::Path(path) };
        let over = self.expr(&over_expr, None);
        let (c, elem) = self.check_pattern(pattern, name, pack, want, spread_at);
        self.refuse_uncopyable(c.id, pattern.span);
        let entry = ty::each(pack.clone(), c.ty.defaulted());
        let kind = hir::ExprKind::Expand { over: over.id, elem, pack: pack.clone(), pattern: c.id };
        self.hir.expr(spread_at, Ty::Tuple(vec![entry]), kind)
    }

    /// A pattern monomorphization cannot copy per element, reported.
    fn refuse_uncopyable(&mut self, e: hir::ExprId, span: Span) {
        if self.copyable_pattern(e) {
            return;
        }
        self.cx.error_help(
            codes::BAD_SPREAD,
            span,
            "this pattern cannot be repeated over a pack",
            "a block, a branch, `or` or a formatting macro in a pattern",
            "a pattern repeated per element is made of calls, operators, fields and literals; \
             put the rest in a function the pattern calls (spec §8.7)",
        );
    }

    /// Whether every node of a pattern is one monomorphization can copy per element.
    fn copyable_pattern(&self, e: hir::ExprId) -> bool {
        let mut ok = true;
        let kind = self.hir.kind(e).clone();
        let mut children = Vec::new();
        if hir::remap_children(&kind, &mut |c| {
            children.push(c);
            c
        })
        .is_none()
        {
            return false;
        }
        for c in children {
            ok &= self.copyable_pattern(c);
        }
        ok
    }

    /// A pattern repeated into a variadic slice parameter, each copy an element of the
    /// array the call builds (spec §7.2, §8.7): checked as one variadic argument is.
    pub(super) fn expand_into_slice(&mut self, a: &Expr, elem_ty: &Ty) -> Option<hir::ExprId> {
        let (pattern, name, pack) = self.expansion(a)?;
        let seg = PathSegment {
            span: pattern.span,
            name: Ident { name: name.clone(), span: pattern.span },
            generics: Vec::new(),
        };
        let path = Path { span: pattern.span, segments: vec![seg] };
        let over_expr = Expr { span: pattern.span, kind: ExprKind::Path(path) };
        let over = self.expr(&over_expr, None);
        self.push();
        let elem = self.bind(&name, Ty::Param(pack.element()), false, LocalKind::Binding, a.span);
        let c = self.variadic_element(pattern, elem_ty);
        self.pop();
        self.refuse_uncopyable(c, pattern.span);
        let kind = hir::ExprKind::Expand { over: over.id, elem, pack: pack.clone(), pattern: c };
        let ty = Ty::Tuple(vec![ty::each(pack, elem_ty.clone())]);
        Some(self.hir.expr(a.span, ty, kind).id)
    }

    /// The arguments as slots, each tuple spread replaced by its elements, or by one entry
    /// for a pack's tuple. A spread that cannot be one stands for nothing, and is reported
    /// when `report` is set: inference reads the arguments before the call checks them.
    pub(super) fn slots<'e>(&mut self, args: &'e [Expr], report: bool) -> Vec<Slot<'e>> {
        let mut out = Vec::with_capacity(args.len());
        for a in args {
            if let Some((pattern, name, pack)) = self.expansion(a) {
                out.push(Slot::Expand { spread: a, pattern, name, pack });
                continue;
            }
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
                // `(i32, ..T, bool)`: each element before the pack, the pack's run, then each
                // element after it counted from the back. The tuple is taken once, by the
                // first, so these are runs of one spread rather than elements of their own.
                Some((at, _)) => {
                    let n = parts.len();
                    let first = out.len();
                    for (index, ty) in parts[..at].iter().enumerate() {
                        out.push(Slot::Run {
                            spread: a,
                            tuple,
                            from: TupleEnd::Front(index),
                            to: TupleEnd::Front(index + 1),
                            ty: ty.clone(),
                            node: ty.clone(),
                            lead: false,
                        });
                    }
                    out.push(Slot::Run {
                        spread: a,
                        tuple,
                        from: TupleEnd::Front(at),
                        to: TupleEnd::Back(n - at - 1),
                        ty: parts[at].clone(),
                        node: Ty::Tuple(vec![parts[at].clone()]),
                        lead: false,
                    });
                    for (j, ty) in parts[at + 1..].iter().enumerate() {
                        let back = n - at - 1 - j;
                        out.push(Slot::Run {
                            spread: a,
                            tuple,
                            from: TupleEnd::Back(back),
                            to: TupleEnd::Back(back - 1),
                            ty: ty.clone(),
                            node: ty.clone(),
                            lead: false,
                        });
                    }
                    if let Some(Slot::Run { lead, .. }) = out.get_mut(first) {
                        *lead = true;
                    }
                }
            }
        }
        out
    }

    /// The type a slot gives the list it stands in.
    fn slot_ty(&mut self, s: &Slot) -> Ty {
        match s {
            Slot::Arg(a) => self.expr_quiet(a).defaulted(),
            Slot::Elem { ty, .. } | Slot::Run { ty, .. } => ty.clone(),
            Slot::Expand { pattern, name, pack, .. } => {
                let (pattern, name, pack) = (*pattern, name.clone(), pack.clone());
                ty::each(pack.clone(), self.pattern_ty(pattern, &name, &pack))
            }
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
            (Slot::Expand { spread, pattern, name, pack }, _) => {
                // What each copy is held to: the element of the entry the list wants.
                let each_want = match want {
                    Some(Ty::Each(e)) if e.pack == *pack => Some(e.ty.clone()),
                    Some(Ty::Param(p)) if p == pack => Some(Ty::Param(pack.element())),
                    _ => None,
                };
                let c = self.expand_node(spread.span, pattern, name, pack, each_want.as_ref());
                if let (Some(w), Ty::Tuple(entry)) = (want.filter(|_| how != Want::Hint), &c.ty) {
                    if let Some(e) = entry.first() {
                        self.expect(w, e, spread.span);
                    }
                }
                c
            }
            (Slot::Elem { spread, tuple, index, ty }, _) => {
                let base = self.expr(tuple, None);
                if let Some(w) = want.filter(|_| how != Want::Hint) {
                    self.expect(w, ty, spread.span);
                }
                let kind = hir::ExprKind::TupleIndex { base: base.id, index: *index };
                self.hir.expr(spread.span, ty.clone(), kind)
            }
            (Slot::Run { spread, tuple, from, to, ty, node, lead }, _) => {
                if let Some(w) = want.filter(|_| how != Want::Hint) {
                    self.expect(w, ty, spread.span);
                }
                let key = (spread.span.start, spread.span.end);
                let (from, to) = (*from, *to);
                if !*lead {
                    if let Some(&of) = self.spread_leads.get(&key) {
                        let kind = hir::ExprKind::SpreadRest { of, from, to };
                        return self.hir.expr(spread.span, node.clone(), kind);
                    }
                }
                let base = self.expr(tuple, None);
                let kind = hir::ExprKind::Spread { base: base.id, from, to };
                let c = self.hir.expr(spread.span, node.clone(), kind);
                self.spread_leads.insert(key, c.id);
                c
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
            if !p.ty.has_params() || s.is_pack() {
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

    /// `countof(A)`: the number of types the pack `A` holds, a query of its tuple that
    /// monomorphization answers (spec §8.7).
    pub(super) fn count_query(&mut self, arg: &Expr) -> (Ty, hir::ExprKind) {
        let written = match &arg.kind {
            ExprKind::Path(p) => {
                Some(dz_ast::Type { span: arg.span, kind: dz_ast::TypeKind::Path(p.clone()) })
            }
            _ => None,
        };
        if let Some(pack) = written.as_ref().and_then(|t| self.cx.pack_named(t)) {
            let query = hir::TypeQuery::Count;
            return (query.result(), hir::ExprKind::TypeQuery { ty: Ty::Tuple(vec![pack]), query });
        }
        let shown = written.as_ref().map(dz_ast::dump::type_str).unwrap_or_else(|| "…".into());
        self.cx.error_help(
            codes::BAD_PACK_USE,
            arg.span,
            format!("`countof` takes a pack, and `{shown}` is not one"),
            "not a pack",
            "`countof(A)` is how many types the pack `A`, declared `<..A>`, holds; a tuple's \
             length is known where its type is written out (spec §8.7)",
        );
        (Ty::Error, hir::ExprKind::Error)
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
        ok
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
        if let Some(s) = slots.iter().find(|s| s.is_pack()) {
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
            tys.push(match (s, &c.ty) {
                (Slot::Run { ty, .. }, _) => ty.clone(),
                (Slot::Expand { .. }, Ty::Tuple(entry)) if entry.len() == 1 => entry[0].clone(),
                _ => c.ty.defaulted(),
            });
            ids.push(c.id);
        }
        self.mk(e, Ty::Tuple(tys), hir::ExprKind::Tuple(ids))
    }
}

/// Every name an expression uses alone, a single segment: what a pattern may name a pack's
/// tuple by (spec §8.7). A nested spread is a pattern of its own, so its names are not.
fn names_in_expr(e: &Expr, out: &mut Vec<String>) {
    let mut go = |x: &Expr| names_in_expr(x, out);
    match &e.kind {
        ExprKind::Path(p) if p.segments.len() == 1 => out.push(p.segments[0].name.name.clone()),
        ExprKind::Call { callee, args } => {
            go(callee);
            args.iter().for_each(go);
        }
        ExprKind::Field { base, .. } | ExprKind::Deref(base) | ExprKind::Move(base) => go(base),
        ExprKind::Index { base, index } => {
            go(base);
            go(index);
        }
        ExprKind::Unary { operand, .. } => go(operand),
        ExprKind::Binary { lhs, rhs, .. } => {
            go(lhs);
            go(rhs);
        }
        ExprKind::Cast { expr, .. } | ExprKind::Ref { expr, .. } => go(expr),
        ExprKind::Tuple(xs) | ExprKind::ArrayLit(xs) => xs.iter().for_each(go),
        ExprKind::StructLit { fields, .. } => fields.iter().for_each(|f| go(&f.value)),
        ExprKind::Named { value, .. } => go(value),
        // What a pattern may not be made of is still a pattern, refused as one, so a name
        // inside it counts: `..(args or x)`, `..if c(args) { … }`, `..format!("{}", args)`.
        ExprKind::Or { expr, .. } => go(expr),
        ExprKind::Intrinsic { args, .. } => args.iter().for_each(go),
        ExprKind::If(i) => go(&i.cond),
        ExprKind::MacroCall(m) => out.extend(
            m.tokens
                .iter()
                .filter(|t| t.text.starts_with(|c: char| c.is_alphabetic() || c == '_'))
                .map(|t| t.text.clone()),
        ),
        // A nested spread, `..x`, is a pattern of its own.
        ExprKind::Range { start: None, end: Some(_), inclusive: false } => {}
        ExprKind::Range { start, end, .. } => {
            start.iter().for_each(|x| go(x));
            end.iter().for_each(|x| go(x));
        }
        _ => {}
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
