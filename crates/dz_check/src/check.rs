//! Checking function bodies: statements, expressions, and the rules of spec §9–§11.

use std::collections::HashMap;
use std::rc::Rc;

use dz_ast::*;
use dz_diag::codes;
use dz_resolve::{SymbolId, SymbolKind};
use dz_span::Span;

use crate::ty::{assignable, join, IntTy, Ty};
use crate::{parse_int_literal, Ctx, FnSig, ItemSig};

pub(crate) fn item(cx: &mut Ctx, it: &Item) {
    match &it.kind {
        ItemKind::Fn(f) => {
            if f.generics.is_empty() {
                let sig = cx.symbol_of(it).and_then(|s| match cx.table.get(s) {
                    Some(ItemSig::Fn(sig)) => Some(sig.clone()),
                    _ => None,
                });
                if let Some(sig) = sig {
                    Body::new(cx, None, sig.ret.clone()).function(f, &sig);
                }
            }
        }
        ItemKind::Struct(s) => {
            let Some(sym) = cx.symbol_of(it) else { return };
            if !s.generics.is_empty() {
                return; // generic bodies are M6
            }
            check_members(cx, sym, &s.members);
        }
        ItemKind::Enum(e) => check_enum(cx, it, e),
        ItemKind::Const(c) => {
            let declared = match cx.symbol_of(it).and_then(|s| cx.table.get(s)) {
                Some(ItemSig::Const(t)) => t.clone(),
                _ => Ty::Error,
            };
            let mut body = Body::new(cx, None, Ty::Void);
            let actual = body.expr(&c.value, Some(&declared));
            if !declared.is_error() {
                body.expect(&declared, &actual, c.value.span);
            }
        }
        // Union payload types are checked during signature collection; trait bodies,
        // macros and generic items belong to later milestones.
        ItemKind::Union(_)
        | ItemKind::Trait(_)
        | ItemKind::Macro(_)
        | ItemKind::MacroInvocation { .. }
        | ItemKind::Error => {}
    }
}

fn check_members(cx: &mut Ctx, sym: SymbolId, members: &[Member]) {
    for m in members {
        match &m.kind {
            MemberKind::Field(f) => {
                if let Some(default) = &f.default {
                    let expected = cx
                        .table
                        .struct_sig(sym)
                        .and_then(|s| s.fields.iter().find(|x| x.name == f.name.name))
                        .map(|x| x.ty.clone())
                        .unwrap_or(Ty::Error);
                    let mut body = Body::new(cx, Some(sym), Ty::Void);
                    let actual = body.expr(default, Some(&expected));
                    body.expect(&expected, &actual, default.span);
                }
            }
            MemberKind::Fn(f) => {
                if !f.generics.is_empty() {
                    continue;
                }
                let sig = cx.table.struct_sig(sym).and_then(|s| s.methods.get(&f.name.name)).cloned();
                if let Some(sig) = sig {
                    Body::new(cx, Some(sym), sig.ret.clone()).function(f, &sig);
                }
            }
            MemberKind::Ctor(f) | MemberKind::Copy(f) | MemberKind::Drop(f) => {
                // Constructors and `copy`/`drop` run on the object, so `self` is mutable.
                let sig = FnSig {
                    params: f
                        .params
                        .iter()
                        .map(|p| crate::ParamSig {
                            name: p.name.name.clone(),
                            ty: cx.lower_type(&p.ty),
                            has_default: p.default.is_some(),
                        })
                        .collect(),
                    ret: Ty::Void,
                    takes_self: Some(true),
                    is_mut: true,
                    vis: m.vis,
                    span: f.name.span,
                    generic: false,
                };
                Body::new(cx, Some(sym), Ty::Void).function(f, &sig);
            }
            MemberKind::TypeAlias { .. } | MemberKind::Error => {}
        }
    }
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
        let ExprKind::Int(text) = &value.kind else {
            cx.error_help(
                codes::TYPE_MISMATCH,
                value.span,
                "enum values must be integer constants",
                "not a constant integer",
                "enums are named integers (spec §7.4)",
            );
            continue;
        };
        if let Some(n) = parse_int_literal(text) {
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
}

struct Body<'a, 'c> {
    cx: &'c mut Ctx<'a>,
    scopes: Vec<HashMap<String, Local>>,
    /// The enclosing function's return type, for `return` and `try`.
    ret: Ty,
    self_ty: Option<SymbolId>,
    self_mutable: bool,
}

impl<'a, 'c> Body<'a, 'c> {
    fn new(cx: &'c mut Ctx<'a>, self_ty: Option<SymbolId>, ret: Ty) -> Body<'a, 'c> {
        Body { cx, scopes: Vec::new(), ret, self_ty, self_mutable: false }
    }

    // --- scopes -----------------------------------------------------------

    fn push(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop(&mut self) {
        self.scopes.pop();
    }

    fn bind(&mut self, name: &str, ty: Ty, mutable: bool) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.to_owned(), Local { ty, mutable });
        }
    }

    fn local(&self, name: &str) -> Option<&Local> {
        self.scopes.iter().rev().find_map(|s| s.get(name))
    }

    /// `Self` means the enclosing type.
    fn subst(&self, t: &Ty) -> Ty {
        match t {
            Ty::SelfTy => match self.self_ty {
                Some(s) => Ty::Named(s),
                None => Ty::Error,
            },
            Ty::Ptr(i) => Ty::Ptr(Rc::new(self.subst(i))),
            Ty::OwnPtr(i) => Ty::OwnPtr(Rc::new(self.subst(i))),
            Ty::Ref { mutable, inner } => {
                Ty::Ref { mutable: *mutable, inner: Rc::new(self.subst(inner)) }
            }
            Ty::Optional(i) => Ty::Optional(Rc::new(self.subst(i))),
            Ty::Fallible(i) => Ty::Fallible(Rc::new(self.subst(i))),
            other => other.clone(),
        }
    }

    fn expect(&mut self, expected: &Ty, actual: &Ty, span: Span) {
        let expected = self.subst(expected);
        let actual = self.subst(actual);
        if assignable(&expected, &actual) {
            return;
        }
        let e = self.cx.name_of(&expected);
        let a = self.cx.name_of(&actual);
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

    fn function(&mut self, f: &FnDecl, sig: &FnSig) {
        self.push();
        if let Some(is_mut) = sig.takes_self {
            self.self_mutable = is_mut;
            if let Some(sym) = self.self_ty {
                self.bind("self", Ty::Named(sym), is_mut);
                // A method body sees its struct's fields unqualified (spec §7.3).
                let fields: Vec<(String, Ty)> = self
                    .cx
                    .table
                    .struct_sig(sym)
                    .map(|s| s.fields.iter().map(|x| (x.name.clone(), x.ty.clone())).collect())
                    .unwrap_or_default();
                for (name, ty) in fields {
                    self.bind(&name, ty, is_mut);
                }
            }
        }
        for (p, ps) in f.params.iter().zip(&sig.params) {
            if let Some(default) = &p.default {
                let actual = self.expr(default, Some(&ps.ty));
                self.expect(&ps.ty, &actual, default.span);
            }
            // Parameters are immutable bindings; mutate through `&mut` or `^`.
            self.bind(&ps.name, ps.ty.clone(), false);
        }
        if let Some(body) = &f.body {
            self.block(body, None);
        }
        self.pop();
    }

    // --- statements -------------------------------------------------------

    /// The type a block produces: its final expression statement, or `void`.
    fn block(&mut self, b: &Block, expected: Option<&Ty>) -> Ty {
        self.push();
        let mut result = Ty::Void;
        let last = b.stmts.len().saturating_sub(1);
        for (i, s) in b.stmts.iter().enumerate() {
            if i == last {
                if let StmtKind::Expr(e) = &s.kind {
                    result = self.expr(e, expected);
                    self.check_unused(&result, e);
                    continue;
                }
            }
            self.stmt(s);
        }
        self.pop();
        result
    }

    fn stmt(&mut self, s: &Stmt) {
        match &s.kind {
            StmtKind::Let(l) => {
                let declared = l.ty.as_ref().map(|t| {
                    let t = self.cx.lower_type(t);
                    self.subst(&t)
                });
                let ty = match (&declared, &l.init) {
                    (Some(d), Some(init)) => {
                        let actual = self.expr(init, Some(d));
                        self.expect(d, &actual, init.span);
                        d.clone()
                    }
                    (Some(d), None) => d.clone(),
                    (None, Some(init)) => {
                        let t = self.expr(init, None);
                        if matches!(t, Ty::Void) {
                            self.cx.error(
                                codes::TYPE_MISMATCH,
                                init.span,
                                "cannot bind a value of type `void`",
                                "this expression produces no value",
                            );
                        }
                        t.defaulted()
                    }
                    // The parser already reported `let` with neither.
                    (None, None) => Ty::Error,
                };
                self.bind_pattern(&l.pat, &ty, l.mutable);
            }
            StmtKind::Assign { target, op, value } => {
                let target_ty = self.expr(target, None);
                let value_ty = self.expr(value, Some(&target_ty));
                self.require_mutable(target);
                if *op == AssignOp::Assign {
                    self.expect(&target_ty, &value_ty, value.span);
                } else {
                    // Compound assignment desugars to `a = a op b` (spec §11.2).
                    if !target_ty.is_wildcard() && !target_ty.is_numeric() {
                        let t = self.cx.name_of(&target_ty);
                        let o = op.as_str();
                        self.cx.error(
                            codes::INVALID_OPERANDS,
                            target.span,
                            format!("`{o}` cannot be applied to `{t}`"),
                            "not a numeric type",
                        );
                    }
                    self.expect(&target_ty, &value_ty, value.span);
                }
            }
            StmtKind::Expr(e) => {
                let t = self.expr(e, None);
                self.check_unused(&t, e);
            }
            StmtKind::While(w) => {
                let c = self.expr(&w.cond, Some(&Ty::Bool));
                self.require_bool(&c, w.cond.span, "while");
                self.block(&w.body, None);
            }
            StmtKind::For(f) => {
                let iter = self.expr(&f.iter, None);
                let elem = match iter.element() {
                    Some(e) => e.clone(),
                    None => {
                        if !iter.is_wildcard() {
                            let t = self.cx.name_of(&iter);
                            self.cx.error_help(
                                codes::TYPE_MISMATCH,
                                f.iter.span,
                                format!("`{t}` cannot be iterated"),
                                "not iterable",
                                "`for` works over ranges, arrays and slices; user types need \
                                 the `Iterate` trait, which arrives with generics (spec §8.5)",
                            );
                        }
                        Ty::Error
                    }
                };
                self.push();
                // `for i, e in &xs` binds an index alongside the element (spec §10.4).
                if f.bindings.len() == 2 {
                    self.bind_pattern(&f.bindings[0], &Ty::Int(IntTy::Usize), false);
                    self.bind_pattern(&f.bindings[1], &elem, false);
                } else {
                    for b in &f.bindings {
                        self.bind_pattern(b, &elem, false);
                    }
                }
                self.block(&f.body, None);
                self.pop();
            }
            StmtKind::Defer(inner) => self.stmt(inner),
            // Nested declarations are checked by the top-level walk.
            StmtKind::Item(_) | StmtKind::Error => {}
        }
    }

    fn bind_pattern(&mut self, p: &Pat, ty: &Ty, mutable: bool) {
        match &p.kind {
            PatKind::Ident(i) => self.bind(&i.name, ty.clone(), mutable),
            PatKind::Tuple(elems) => match ty {
                Ty::Tuple(parts) if parts.len() == elems.len() => {
                    for (e, t) in elems.iter().zip(parts) {
                        self.bind_pattern(e, t, mutable);
                    }
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
                    for e in elems {
                        self.bind_pattern(e, &Ty::Error, mutable);
                    }
                }
            },
            PatKind::Wild | PatKind::Error => {}
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

    /// Discarding a `!T` is an error; discarding a `?T` is a warning (spec §9.7).
    fn check_unused(&mut self, t: &Ty, e: &Expr) {
        // `try` and `with` already consume the result.
        if matches!(e.kind, ExprKind::Try(_) | ExprKind::With(_) | ExprKind::Catch { .. }) {
            return;
        }
        match t {
            Ty::Fallible(_) => {
                let name = self.cx.name_of(t);
                self.cx.error_help(
                    codes::UNUSED_RESULT,
                    e.span,
                    format!("unused `{name}`"),
                    "this may fail, and the failure is discarded",
                    "handle it with `with`, propagate it with `try`, supply a fallback with \
                     `catch`, or write `let _ = ...` to ignore it deliberately (spec §9.7)",
                );
            }
            Ty::Optional(_) => {
                let name = self.cx.name_of(t);
                self.cx.diags.push(
                    dz_diag::Diagnostic::warning(
                        codes::UNUSED_RESULT,
                        format!("unused `{name}`"),
                    )
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
            Some(true) => {}
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
                    Some(l) => Some(l.mutable),
                    // A module-level `const` is never assignable.
                    None => Some(false),
                }
            }
            // Through a pointer, mutability comes from the pointer, not the binding.
            ExprKind::Deref(inner) => {
                let t = self.expr_quiet(inner);
                Some(matches!(t, Ty::Ptr(_) | Ty::OwnPtr(_)))
            }
            ExprKind::Field { base, .. } => {
                let base_ty = self.expr_quiet(base);
                match &base_ty {
                    Ty::Ptr(_) | Ty::OwnPtr(_) => Some(true),
                    Ty::Ref { mutable, .. } => Some(*mutable),
                    _ => self.place_mutability(base),
                }
            }
            ExprKind::Index { base, .. } => {
                let base_ty = self.expr_quiet(base);
                match &base_ty {
                    Ty::Ptr(_) | Ty::OwnPtr(_) => Some(true),
                    Ty::Ref { mutable, .. } => Some(*mutable),
                    _ => self.place_mutability(base),
                }
            }
            _ => None,
        }
    }

    fn describe_place(&mut self, e: &Expr) -> (String, String) {
        match &e.kind {
            ExprKind::Path(p) if p.segments.len() == 1 => {
                let name = p.segments[0].name.name.clone();
                if self.local(&name).is_some() {
                    (
                        format!("`{name}`"),
                        format!("declare it as `let mut {name}` to allow assignment"),
                    )
                } else {
                    (format!("`{name}`"), "constants and functions cannot be assigned to".to_owned())
                }
            }
            ExprKind::Field { name, .. } => (
                format!("field `{}`", name.name),
                "a field is mutable through `&mut self` or a `^` pointer; a `fn` method \
                 takes `&self`, so make it `mut fn` (spec §7.3)"
                    .to_owned(),
            ),
            _ => ("this".to_owned(), "not a mutable place".to_owned()),
        }
    }

    /// Type of an expression without emitting diagnostics, for place analysis.
    fn expr_quiet(&mut self, e: &Expr) -> Ty {
        let before = self.cx.diags.len();
        let t = self.expr(e, None);
        self.cx.diags.truncate(before);
        t
    }

    // --- expressions ------------------------------------------------------

    fn expr(&mut self, e: &Expr, expected: Option<&Ty>) -> Ty {
        match &e.kind {
            ExprKind::Int(text) => {
                if let (Some(exp), Some(v)) = (expected, parse_int_literal(text)) {
                    if let Ty::Int(i) = self.subst(exp) {
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
                            return Ty::Error;
                        }
                        return Ty::Int(i);
                    }
                    if let Ty::Float(f) = self.subst(exp) {
                        return Ty::Float(f);
                    }
                }
                Ty::IntLit
            }
            ExprKind::Float(_) => match expected.map(|t| self.subst(t)) {
                Some(Ty::Float(f)) => Ty::Float(f),
                _ => Ty::FloatLit,
            },
            ExprKind::Str(_) | ExprKind::RawStr(_) => Ty::Str,
            ExprKind::Char(_) => Ty::Char,
            ExprKind::Bool(_) => Ty::Bool,
            ExprKind::Null => match expected.map(|t| self.subst(t)) {
                Some(t @ (Ty::Ptr(_) | Ty::OwnPtr(_))) => t,
                _ => Ty::Ptr(Rc::new(Ty::Error)),
            },
            ExprKind::None => match expected.map(|t| self.subst(t)) {
                Some(t @ Ty::Optional(_)) => t,
                _ => {
                    // `none` only means something where a `?T` is wanted (spec §9.1).
                    if let Ty::Optional(inner) = &self.ret {
                        return Ty::Optional(inner.clone());
                    }
                    self.cx.error_help(
                        codes::NO_CONTEXT_FOR_LITERAL,
                        e.span,
                        "`none` needs an optional type here",
                        "nothing here expects a `?T`",
                        "`none` is the empty case of `?T` (spec §9.1)",
                    );
                    Ty::Error
                }
            },
            ExprKind::Path(p) => self.path_expr(p, e.span),
            ExprKind::Tuple(elems) => {
                let parts = match expected.map(|t| self.subst(t)) {
                    Some(Ty::Tuple(want)) if want.len() == elems.len() => {
                        elems.iter().zip(&want).map(|(x, w)| self.expr(x, Some(w))).collect()
                    }
                    _ => elems.iter().map(|x| self.expr(x, None).defaulted()).collect(),
                };
                Ty::Tuple(parts)
            }
            ExprKind::Ref { mutable, expr } => {
                let inner = self.expr(expr, None);
                if *mutable {
                    self.require_mutable(expr);
                }
                Ty::Ref { mutable: *mutable, inner: Rc::new(inner.defaulted()) }
            }
            ExprKind::Deref(inner) => {
                let t = self.expr(inner, None);
                match &t {
                    Ty::Ptr(i) | Ty::OwnPtr(i) => (**i).clone(),
                    _ if t.is_wildcard() => Ty::Error,
                    _ => {
                        let n = self.cx.name_of(&t);
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
                }
            }
            ExprKind::Move(inner) => self.expr(inner, expected),
            ExprKind::Unary { op, operand } => {
                let t = self.expr(operand, expected);
                match op {
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
                            return Ty::Error;
                        }
                        t
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
                            return Ty::Error;
                        }
                        t
                    }
                }
            }
            ExprKind::Binary { op, lhs, rhs } => self.binary(*op, lhs, rhs, e.span, expected),
            ExprKind::Cast { expr, ty } => {
                let from = self.expr(expr, None);
                let target = self.cx.lower_type(ty);
                let target = self.subst(&target);
                self.check_cast(&from, &target, e.span);
                target
            }
            ExprKind::Range { start, end, .. } => {
                let s = start.as_ref().map(|x| self.expr(x, None));
                let en = end.as_ref().map(|x| self.expr(x, None));
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
                Ty::Range(Rc::new(elem))
            }
            ExprKind::Index { base, index } => {
                let b = self.expr(base, None);
                let i = self.expr(index, Some(&Ty::Int(IntTy::Usize)));
                if !i.is_wildcard() && !i.is_integral() {
                    let n = self.cx.name_of(&i);
                    self.cx.error(
                        codes::TYPE_MISMATCH,
                        index.span,
                        format!("an index must be an integer, found `{n}`"),
                        "not an integer",
                    );
                }
                match b.element() {
                    Some(t) => t.clone(),
                    None => {
                        if !b.is_wildcard() {
                            let n = self.cx.name_of(&b);
                            self.cx.error(
                                codes::NOT_INDEXABLE,
                                base.span,
                                format!("`{n}` cannot be indexed"),
                                "not an array or slice",
                            );
                        }
                        Ty::Error
                    }
                }
            }
            ExprKind::Field { base, name } => {
                let b = self.expr(base, None);
                self.field_type(&b, name, base.span)
            }
            ExprKind::Call { callee, args } => self.call(callee, args, e.span),
            ExprKind::StructLit { path, fields } => {
                let ty = self.cx.lower_type(&Type {
                    span: path.span,
                    kind: TypeKind::Path(path.clone()),
                });
                let ty = self.subst(&ty);
                self.struct_literal(&ty, fields, e.span);
                ty
            }
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
                    return Ty::Error;
                };
                self.dot_infer(&want, name, init.as_deref(), e.span);
                want
            }
            ExprKind::Block(b) => self.block(b, expected),
            ExprKind::If(i) => {
                let c = self.expr(&i.cond, Some(&Ty::Bool));
                self.require_bool(&c, i.cond.span, "if");
                let then = self.block(&i.then, expected);
                match &i.else_branch {
                    Some(else_expr) => {
                        let other = self.expr(else_expr, expected);
                        join(&then, &other).unwrap_or(Ty::Void)
                    }
                    // Without an `else` an `if` cannot produce a value.
                    None => Ty::Void,
                }
            }
            ExprKind::Switch(s) => self.switch(s, expected),
            ExprKind::With(w) => self.with(w, expected),
            ExprKind::Try(inner) => self.try_expr(inner, e.span),
            ExprKind::Catch { expr, handler } => {
                let subject = self.expr(expr, None);
                let (payload, failure) = match &subject {
                    Ty::Fallible(inner) => ((**inner).clone(), Some(Ty::Error)),
                    Ty::Optional(inner) => ((**inner).clone(), None),
                    _ if subject.is_wildcard() => (Ty::Error, None),
                    _ => {
                        let n = self.cx.name_of(&subject);
                        self.cx.error_help(
                            codes::TYPE_MISMATCH,
                            expr.span,
                            format!("`catch` needs a `?T` or `!T`, found `{n}`"),
                            "this cannot fail",
                            "`catch` supplies a fallback for a failing expression (spec §9.6)",
                        );
                        (Ty::Error, None)
                    }
                };
                let _ = failure;
                match handler.as_ref() {
                    CatchHandler::Value(v) => {
                        let t = self.expr(v, Some(&payload));
                        self.expect(&payload, &t, v.span);
                    }
                    CatchHandler::Block { binding, body } => {
                        self.push();
                        let name =
                            binding.as_ref().map(|b| b.name.clone()).unwrap_or("it".to_owned());
                        self.bind(&name, Ty::Error, false);
                        let t = self.block(body, Some(&payload));
                        if !matches!(t, Ty::Never | Ty::Void) {
                            self.expect(&payload, &t, body.span);
                        }
                        self.pop();
                    }
                }
                payload
            }
            ExprKind::Return(v) => {
                let ret = self.ret.clone();
                match v {
                    Some(value) => {
                        let t = self.expr(value, Some(&ret));
                        // Returning a plain `T` from a `?T`/`!T` function is the success
                        // case — there is no wrapping step (spec §9.1).
                        let target = match &ret {
                            Ty::Optional(inner) | Ty::Fallible(inner)
                                if !assignable(&ret, &t) =>
                            {
                                (**inner).clone()
                            }
                            other => other.clone(),
                        };
                        self.expect(&target, &t, value.span);
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
                Ty::Never
            }
            ExprKind::Break { value, .. } => {
                if let Some(v) = value {
                    self.expr(v, None);
                }
                Ty::Never
            }
            ExprKind::Continue { .. } => Ty::Never,
            ExprKind::Intrinsic { name, args } => {
                for a in args {
                    self.expr(a, None);
                }
                // spec §12.7
                match name.name.as_str() {
                    "line" | "column" => Ty::Int(IntTy::U32),
                    "file" | "function" | "module" | "expr_text" => Ty::Str,
                    _ => Ty::Error,
                }
            }
            // Macro bodies are opaque until M10.
            ExprKind::MacroCall { .. } | ExprKind::Error => Ty::Error,
        }
    }

    // --- sub-checks -------------------------------------------------------

    fn binary(
        &mut self,
        op: BinaryOp,
        lhs: &Expr,
        rhs: &Expr,
        span: Span,
        expected: Option<&Ty>,
    ) -> Ty {
        use BinaryOp::*;
        match op {
            And | Or => {
                let a = self.expr(lhs, Some(&Ty::Bool));
                let b = self.expr(rhs, Some(&Ty::Bool));
                self.require_bool(&a, lhs.span, op.as_str());
                self.require_bool(&b, rhs.span, op.as_str());
                Ty::Bool
            }
            Eq | Ne | Lt | Gt | Le | Ge => {
                let a = self.expr(lhs, None);
                let b = self.expr(rhs, Some(&a));
                if join(&a, &b).is_none() {
                    let (x, y) = (self.cx.name_of(&a), self.cx.name_of(&b));
                    self.cx.error_help(
                        codes::INVALID_OPERANDS,
                        span,
                        format!("cannot compare `{x}` with `{y}`"),
                        "operands have different types",
                        "Dazzle has no implicit conversions (spec §5.1) — cast one side",
                    );
                }
                Ty::Bool
            }
            BitAnd | BitOr | BitXor | Shl | Shr => {
                let a = self.expr(lhs, expected);
                let b = self.expr(rhs, Some(&a));
                for (t, e) in [(&a, lhs), (&b, rhs)] {
                    if !t.is_wildcard() && !t.is_integral() {
                        let n = self.cx.name_of(t);
                        let o = op.as_str();
                        self.cx.error(
                            codes::INVALID_OPERANDS,
                            e.span,
                            format!("`{o}` needs integer operands, found `{n}`"),
                            "not an integer",
                        );
                        return Ty::Error;
                    }
                }
                join(&a, &b).unwrap_or(Ty::Error)
            }
            Add | Sub | Mul | Div | Rem => {
                let a = self.expr(lhs, expected);
                let b = self.expr(rhs, Some(&a));
                for (t, e) in [(&a, lhs), (&b, rhs)] {
                    if !t.is_wildcard() && !t.is_numeric() {
                        let n = self.cx.name_of(t);
                        let o = op.as_str();
                        self.cx.error(
                            codes::INVALID_OPERANDS,
                            e.span,
                            format!("`{o}` cannot be applied to `{n}`"),
                            "not a numeric type",
                        );
                        return Ty::Error;
                    }
                }
                match join(&a, &b) {
                    Some(t) => t,
                    None => {
                        let (x, y) = (self.cx.name_of(&a), self.cx.name_of(&b));
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
                }
            }
        }
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
            // An enum converts to its backing type (spec §7.4).
            (Ty::Named(id), b) if b.is_integral() => {
                matches!(self.cx.table.get(*id), Some(ItemSig::Enum { .. }))
            }
            _ => matches!(
                (from, to),
                (Ty::Ptr(_) | Ty::OwnPtr(_), Ty::Ptr(_))
                    | (Ty::Str, Ty::CStr)
                    | (Ty::CStr, Ty::Str)
            ),
        };
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

    fn field_type(&mut self, base: &Ty, name: &Ident, base_span: Span) -> Ty {
        let peeled = base.peeled().clone();
        // Tuple access: `t.0`
        if let Ty::Tuple(parts) = &peeled {
            if let Ok(i) = name.name.parse::<usize>() {
                return match parts.get(i) {
                    Some(t) => t.clone(),
                    None => {
                        let n = parts.len();
                        self.cx.error(
                            codes::NO_SUCH_FIELD,
                            name.span,
                            format!("this tuple has {n} elements"),
                            format!("no element `{i}`"),
                        );
                        Ty::Error
                    }
                };
            }
        }
        // Slices carry a length (spec §5.3).
        if let Ty::Array { .. } = &peeled {
            if name.name == "len" {
                return Ty::Int(IntTy::Usize);
            }
        }
        if peeled.is_wildcard() {
            return Ty::Error;
        }
        let Some(sym) = peeled.named() else {
            let t = self.cx.name_of(&peeled);
            self.cx.error(
                codes::NO_SUCH_FIELD,
                name.span,
                format!("`{t}` has no field `{}`", name.name),
                "not a struct",
            );
            return Ty::Error;
        };
        if self.cx.table.is_generic(sym) {
            return Ty::Error; // generic members wait for M6
        }
        let Some(sig) = self.cx.table.struct_sig(sym) else {
            let t = self.cx.name_of(&peeled);
            self.cx.error(
                codes::NO_SUCH_FIELD,
                name.span,
                format!("`{t}` has no fields"),
                "not a struct",
            );
            return Ty::Error;
        };
        let Some(field) = sig.fields.iter().find(|f| f.name == name.name).cloned() else {
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
                d = d.with_help(format!("`{}` is a method — call it as `.{}()`", name.name, name.name));
            } else if let Some(s) = suggestion {
                d = d.with_help(format!("did you mean `{s}`?"));
            }
            self.cx.diags.push(d);
            let _ = base_span;
            return Ty::Error;
        };
        self.check_member_visible(sym, &field.name, field.vis, field.span, name.span, "field");
        field.ty
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

        // Inside the struct's own methods everything is reachable (spec §4).
        if self.self_ty == Some(owner) {
            return;
        }
        let ok = match vis {
            Vis::Public => true,
            Vis::Module | Vis::Private => owner_module == self.cx.module && vis == Vis::Module,
        };
        if ok {
            return;
        }
        let module_name = self.cx.ws.module(owner_module).name.clone();
        let help = if vis == Vis::Module {
            format!("`pub(module)` is visible inside module `{module_name}` only; mark it `pub` to export it (spec §4)")
        } else {
            format!("this {what} is private to `{owner_name}`; mark it `pub` or `pub(module)` (spec §4)")
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

    fn call(&mut self, callee: &Expr, args: &[Expr], span: Span) -> Ty {
        // A method call parses as a call on a field access.
        if let ExprKind::Field { base, name } = &callee.kind {
            let base_ty = self.expr(base, None);
            let peeled = base_ty.peeled().clone();
            if let Some(sym) = peeled.named() {
                if !self.cx.table.is_generic(sym) {
                    if let Some(sig) =
                        self.cx.table.struct_sig(sym).and_then(|s| s.methods.get(&name.name)).cloned()
                    {
                        self.check_member_visible(
                            sym, &name.name, sig.vis, sig.span, name.span, "method",
                        );
                        if sig.is_mut {
                            self.require_mutable_receiver(base, &base_ty, &name.name);
                        }
                        self.check_args(&sig, args, span, &name.name);
                        return self.subst(&sig.ret);
                    }
                    // Not a method: fall through so the field-access path reports.
                    let sig_has_field = self
                        .cx
                        .table
                        .struct_sig(sym)
                        .map(|s| s.fields.iter().any(|f| f.name == name.name))
                        .unwrap_or(false);
                    if !sig_has_field {
                        let owner = self.cx.res.symbol(sym).name.clone();
                        let names: Vec<String> = self
                            .cx
                            .table
                            .struct_sig(sym)
                            .map(|s| s.methods.keys().cloned().collect())
                            .unwrap_or_default();
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
                        return Ty::Error;
                    }
                }
            }
        }

        // A constructor call: `Buffer(8)`.
        if let ExprKind::Path(p) = &callee.kind {
            if let Some(sym) = self.path_symbol(p) {
                if self.cx.res.symbol(sym).kind == SymbolKind::Struct
                    && !self.cx.table.is_generic(sym)
                {
                    let ctors =
                        self.cx.table.struct_sig(sym).map(|s| s.ctors.clone()).unwrap_or_default();
                    if let Some(sig) = ctors.first().cloned() {
                        let name = self.cx.res.symbol(sym).name.clone();
                        self.check_args(&sig, args, span, &name);
                        return Ty::Named(sym);
                    }
                }
            }
            // A named function: use its signature, which knows about default arguments
            // (spec §7.2). The generic `Ty::Fn` path below cannot, since a function type
            // does not record which parameters have defaults.
            if let Some(sym) = self.path_symbol(p) {
                if let Some(ItemSig::Fn(sig)) = self.cx.table.get(sym) {
                    if !sig.generic {
                        let sig = sig.clone();
                        let name = self.cx.res.symbol(sym).name.clone();
                        self.check_args(&sig, args, span, &name);
                        return self.subst(&sig.ret);
                    }
                }
            }

            // Builtins are variadic and untyped until the stdlib exists.
            if p.segments.len() == 1 && dz_resolve::BUILTIN_FNS.contains(&p.segments[0].name.name.as_str())
            {
                for a in args {
                    self.expr(a, None);
                }
                return builtin_call_type(&p.segments[0].name.name);
            }
        }

        let callee_ty = self.expr(callee, None);
        match &callee_ty {
            Ty::Fn { params, ret } => {
                if params.len() != args.len() {
                    let (want, got) = (params.len(), args.len());
                    self.cx.error(
                        codes::WRONG_ARG_COUNT,
                        span,
                        format!("expected {want} {}, found {got}", plural(want, "argument")),
                        "wrong number of arguments",
                    );
                }
                for (a, p) in args.iter().zip(params) {
                    let t = self.expr(a, Some(p));
                    self.expect(p, &t, a.span);
                }
                (**ret).clone()
            }
            _ if callee_ty.is_wildcard() => {
                for a in args {
                    self.expr(a, None);
                }
                Ty::Error
            }
            _ => {
                let n = self.cx.name_of(&callee_ty);
                self.cx.error(
                    codes::NOT_CALLABLE,
                    callee.span,
                    format!("`{n}` is not callable"),
                    "not a function",
                );
                for a in args {
                    self.expr(a, None);
                }
                Ty::Error
            }
        }
    }

    fn require_mutable_receiver(&mut self, base: &Expr, base_ty: &Ty, method: &str) {
        let ok = match base_ty {
            Ty::Ref { mutable, .. } => *mutable,
            Ty::Ptr(_) | Ty::OwnPtr(_) => true,
            _ => self.place_mutability(base).unwrap_or(true),
        };
        if !ok {
            self.cx.error_help(
                codes::NOT_MUTABLE,
                base.span,
                format!("`{method}` needs a mutable receiver"),
                "this is not mutable",
                "`mut fn` methods take `&mut self` — bind with `let mut`, or take `&mut` here \
                 (spec §7.3)",
            );
        }
    }

    fn check_args(&mut self, sig: &FnSig, args: &[Expr], span: Span, name: &str) {
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
        for (a, p) in args.iter().zip(&sig.params) {
            let want = self.subst(&p.ty);
            let t = self.expr(a, Some(&want));
            self.expect(&want, &t, a.span);
        }
        for a in args.iter().skip(sig.params.len()) {
            self.expr(a, None);
        }
    }

    fn path_symbol(&self, p: &Path) -> Option<SymbolId> {
        let first = p.segments.first()?;
        if p.segments.len() == 1 {
            return self.cx.res.lookup_in_file(self.cx.module, self.cx.file, &first.name.name);
        }
        let target = self.cx.res.module_alias(self.cx.file, &first.name.name)?;
        self.cx.res.lookup(target, &p.segments.get(1)?.name.name)
    }

    fn path_expr(&mut self, p: &Path, span: Span) -> Ty {
        let Some(first) = p.segments.first() else { return Ty::Error };

        // A local wins over everything for a bare name.
        if p.segments.len() == 1 {
            if let Some(l) = self.local(&first.name.name) {
                return l.ty.clone();
            }
            if dz_resolve::BUILTIN_FNS.contains(&first.name.name.as_str()) {
                return Ty::Error; // only meaningful when called
            }
        }

        let Some(sym) = self.path_symbol(p) else { return Ty::Error };
        let symbol_kind = self.cx.res.symbol(sym).kind;

        // `Type::Variant` and `module::Type::Variant`
        let tail = if p.segments.len() > 1 && self.cx.res.module_alias(self.cx.file, &first.name.name).is_some() {
            &p.segments[2..]
        } else {
            &p.segments[1..]
        };

        if let Some(seg) = tail.first() {
            return self.assoc_item(sym, seg, span);
        }

        match symbol_kind {
            SymbolKind::Fn => match self.cx.table.get(sym) {
                Some(ItemSig::Fn(sig)) => Ty::Fn {
                    params: sig.params.iter().map(|p| p.ty.clone()).collect(),
                    ret: Rc::new(sig.ret.clone()),
                },
                _ => Ty::Error,
            },
            SymbolKind::Const => match self.cx.table.get(sym) {
                Some(ItemSig::Const(t)) => t.clone(),
                _ => Ty::Error,
            },
            // A bare type name in value position is only meaningful as a constructor
            // call, which `call` handles before reaching here.
            _ => Ty::Named(sym),
        }
    }

    fn assoc_item(&mut self, owner: SymbolId, seg: &PathSegment, _span: Span) -> Ty {
        match self.cx.table.get(owner) {
            Some(ItemSig::Enum { variants, .. }) => {
                if variants.iter().any(|v| v.name == seg.name.name) {
                    Ty::Named(owner)
                } else {
                    Ty::Error // name resolution already reported the unknown variant
                }
            }
            Some(ItemSig::Union { variants }) => {
                if variants.iter().any(|v| v.name == seg.name.name) {
                    Ty::Named(owner)
                } else {
                    Ty::Error
                }
            }
            _ => Ty::Error,
        }
    }

    fn struct_literal(&mut self, ty: &Ty, fields: &[FieldInit], span: Span) {
        let Some(sym) = ty.named() else {
            for f in fields {
                self.expr(&f.value, None);
            }
            return;
        };
        if self.cx.table.is_generic(sym) {
            for f in fields {
                self.expr(&f.value, None);
            }
            return;
        }
        let Some(sig) = self.cx.table.struct_sig(sym).cloned() else {
            for f in fields {
                self.expr(&f.value, None);
            }
            return;
        };
        let owner = self.cx.res.symbol(sym).name.clone();

        let positional = fields.iter().all(|f| f.name.is_none()) && !fields.is_empty();
        if positional {
            // `Vec3{1, 2, 3}` is allowed only when every field is listed (spec §7.3).
            if fields.len() != sig.fields.len() {
                let (want, got) = (sig.fields.len(), fields.len());
                self.cx.error_help(
                    codes::MISSING_FIELD,
                    span,
                    format!("`{owner}` has {want} fields, found {got}"),
                    "wrong number of values",
                    "positional initialisation requires every field; use `name: value` \
                     otherwise (spec §7.3)",
                );
            }
            for (f, decl) in fields.iter().zip(&sig.fields) {
                let want = self.subst(&decl.ty);
                let t = self.expr(&f.value, Some(&want));
                self.expect(&want, &t, f.value.span);
            }
            return;
        }

        let mut seen: Vec<&str> = Vec::new();
        for f in fields {
            let Some(name) = &f.name else {
                self.expr(&f.value, None);
                continue;
            };
            let Some(decl) = sig.fields.iter().find(|x| x.name == name.name) else {
                let names: Vec<String> = sig.fields.iter().map(|x| x.name.clone()).collect();
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
            self.check_member_visible(sym, &decl.name, decl.vis, decl.span, name.span, "field");
            let want = self.subst(&decl.ty);
            let t = self.expr(&f.value, Some(&want));
            self.expect(&want, &t, f.value.span);
        }

        // Anything omitted must have a default (spec §6.2, §7.3).
        let missing: Vec<String> = sig
            .fields
            .iter()
            .filter(|d| !d.has_default && !seen.contains(&d.name.as_str()))
            .map(|d| format!("`{}`", d.name))
            .collect();
        if !missing.is_empty() {
            let list = missing.join(", ");
            self.cx.error_help(
                codes::MISSING_FIELD,
                span,
                format!("missing {} in `{owner}`", if missing.len() == 1 { "field" } else { "fields" }),
                format!("{list} not initialised"),
                "a field with a declared default may be omitted (spec §6.2)",
            );
        }
    }

    fn dot_infer(&mut self, want: &Ty, name: &Ident, init: Option<&[FieldInit]>, span: Span) {
        let Some(sym) = want.named() else { return };
        let known = match self.cx.table.get(sym) {
            Some(ItemSig::Enum { variants, .. }) | Some(ItemSig::Union { variants }) => {
                variants.iter().any(|v| v.name == name.name)
            }
            _ => false,
        };
        if !known {
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
        if let Some(fields) = init {
            for f in fields {
                self.expr(&f.value, None);
            }
        }
        let _ = span;
    }

    fn switch(&mut self, s: &SwitchExpr, expected: Option<&Ty>) -> Ty {
        let subject = self.expr(&s.subject, None);
        let peeled = subject.peeled().clone();
        let sym = peeled.named();

        let variants: Vec<crate::VariantSig> = match sym.and_then(|id| self.cx.table.get(id)) {
            Some(ItemSig::Enum { variants, .. }) | Some(ItemSig::Union { variants }) => {
                variants.clone()
            }
            _ => {
                if !peeled.is_wildcard() {
                    let n = self.cx.name_of(&peeled);
                    self.cx.error_help(
                        codes::TYPE_MISMATCH,
                        s.subject.span,
                        format!("cannot switch on `{n}`"),
                        "not an enum or union",
                        "`switch` matches the variants of an enum or a tagged union \
                         (spec §10.3)",
                    );
                }
                Vec::new()
            }
        };
        let is_union = matches!(
            sym.and_then(|id| self.cx.table.get(id)),
            Some(ItemSig::Union { .. })
        );

        let mut covered: Vec<String> = Vec::new();
        let mut result: Option<Ty> = None;

        for arm in &s.arms {
            self.push();
            for pat in &arm.patterns {
                let known = variants.iter().find(|v| v.name == pat.name.name).cloned();
                match &known {
                    Some(v) => {
                        covered.push(v.name.clone());
                        if let Some(b) = &pat.binding {
                            // The binding is a reference into the union, so matching
                            // never copies (spec §10.3).
                            let payload = if is_union && !v.fields.is_empty() {
                                Ty::Named(sym.unwrap())
                            } else {
                                Ty::Named(sym.unwrap_or(SymbolId(0)))
                            };
                            self.bind(&b.name, payload, true);
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
                            .at(self.cx.file, pat.name.span, "unknown variant");
                            if let Some(x) = suggestion {
                                d = d.with_help(format!("did you mean `{x}`?"));
                            }
                            self.cx.diags.push(d);
                        }
                        if let Some(b) = &pat.binding {
                            self.bind(&b.name, Ty::Error, true);
                        }
                    }
                }
            }
            let t = self.block(&arm.body, expected);
            result = Some(match result {
                Some(prev) => join(&prev, &t).unwrap_or(Ty::Void),
                None => t,
            });
            self.pop();
        }

        if let Some(d) = &s.default {
            let t = self.block(d, expected);
            result = Some(match result {
                Some(prev) => join(&prev, &t).unwrap_or(Ty::Void),
                None => t,
            });
        } else if !variants.is_empty() {
            let missing: Vec<String> = variants
                .iter()
                .filter(|v| !covered.contains(&v.name))
                .map(|v| format!("`{}`", v.name))
                .collect();
            if !missing.is_empty() {
                let owner =
                    sym.map(|id| self.cx.res.symbol(id).name.clone()).unwrap_or_default();
                let list = missing.join(", ");
                self.cx.error_help(
                    codes::NON_EXHAUSTIVE_SWITCH,
                    s.span,
                    format!("`switch` on `{owner}` is not exhaustive"),
                    format!("{list} not covered"),
                    "add the missing arms, or a `default` arm (spec §10.3)",
                );
            }
        }

        result.unwrap_or(Ty::Void)
    }

    fn with(&mut self, w: &WithExpr, expected: Option<&Ty>) -> Ty {
        let subject = self.expr(&w.subject, None);
        let (payload, failure) = match &subject {
            Ty::Optional(inner) => ((**inner).clone(), None),
            Ty::Fallible(inner) => ((**inner).clone(), Some(Ty::Error)),
            _ if subject.is_wildcard() => (Ty::Error, None),
            _ => {
                let n = self.cx.name_of(&subject);
                self.cx.error_help(
                    codes::TYPE_MISMATCH,
                    w.subject.span,
                    format!("`with` needs a `?T` or `!T`, found `{n}`"),
                    "this cannot be absent or fail",
                    "`with` binds the success payload of an optional or fallible value \
                     (spec §9.4)",
                );
                (Ty::Error, None)
            }
        };

        self.push();
        let name = w.binding.as_ref().map(|b| b.name.clone()).unwrap_or("it".to_owned());
        self.bind(&name, payload, false);
        let then = self.block(&w.body, expected);
        self.pop();

        let Some(else_body) = &w.else_body else {
            // Without an `else` the whole thing produces nothing.
            return Ty::Void;
        };

        self.push();
        if let Some(b) = &w.else_binding {
            // `else` binds the failure payload; only `!T` has one (spec §9.4).
            if failure.is_none() && !subject.is_wildcard() {
                self.cx.error_help(
                    codes::TYPE_MISMATCH,
                    b.span,
                    format!("`?T` has no failure value to bind as `{}`", b.name),
                    "nothing to bind here",
                    "only `!T` carries an `Error`; write `else { ... }` with no binding \
                     (spec §9.4)",
                );
            }
            self.bind(&b.name, Ty::Error, false);
        } else {
            self.bind("it", Ty::Error, false);
        }
        let other = self.block(else_body, expected);
        self.pop();

        join(&then, &other).unwrap_or(Ty::Void)
    }

    fn try_expr(&mut self, inner: &Expr, span: Span) -> Ty {
        let t = self.expr(inner, None);
        let payload = match &t {
            Ty::Optional(i) | Ty::Fallible(i) => (**i).clone(),
            _ if t.is_wildcard() => return Ty::Error,
            _ => {
                let n = self.cx.name_of(&t);
                self.cx.error_help(
                    codes::TYPE_MISMATCH,
                    inner.span,
                    format!("`try` needs a `?T` or `!T`, found `{n}`"),
                    "this cannot fail",
                    "`try` propagates a failure to the caller (spec §9.5)",
                );
                return Ty::Error;
            }
        };

        // `try` is only legal where the failure has somewhere to go (spec §9.5).
        let ok = matches!(
            (&t, &self.ret),
            (Ty::Fallible(_), Ty::Fallible(_)) | (Ty::Optional(_), Ty::Optional(_))
        );
        if !ok {
            let operand = self.cx.name_of(&t);
            let ret = self.cx.name_of(&self.ret.clone());
            let needed = if matches!(t, Ty::Fallible(_)) { "!U" } else { "?U" };
            self.cx.error_help(
                codes::TRY_OUTSIDE_FALLIBLE,
                span,
                format!("`try` on a `{operand}` needs an enclosing function returning `{needed}`"),
                format!("this function returns `{ret}`"),
                format!(
                    "change the return type to `{needed}`, or handle it here with `with` \
                     or `catch` (spec §9.5)"
                ),
            );
        }
        payload
    }
}

fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        word.to_owned()
    } else {
        format!("{word}s")
    }
}

/// Return types of the builtin calls (spec §5.5, §9.1, §12.9, §13.1).
fn builtin_call_type(name: &str) -> Ty {
    match name {
        "sizeof" | "alignof" => Ty::Int(IntTy::Usize),
        // `error(...)` is the failure case of whatever `!T` the context wants, so its
        // payload is `Never` and unifies with any success type (spec §9.1).
        "error" => Ty::Fallible(Rc::new(Ty::Never)),
        "format" => Ty::Str,
        "typeid_of" => Ty::Int(IntTy::U32),
        // `panic` never returns; the rest produce nothing useful yet.
        "panic" => Ty::Never,
        _ => Ty::Void,
    }
}
