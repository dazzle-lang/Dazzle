//! Checking function bodies: statements, expressions, and the rules of spec §9–§11.

use std::collections::HashMap;
use std::rc::Rc;

use dz_ast::*;
use dz_diag::codes;
use dz_hir::{self as hir, BlockId, BodyOwner, LocalId, LocalKind, StmtId};
use dz_resolve::{SymbolId, SymbolKind};
use dz_span::Span;

use crate::build::{BodyBuilder, Checked, CheckedBlock};
use crate::ty::{assignable, join, IntTy, Ty};
use crate::{
    decode_raw_string_literal, decode_string_literal, parse_float_literal, parse_int_literal, Ctx,
    FnSig, ItemSig,
};

pub(crate) fn item(cx: &mut Ctx, it: &Item) {
    match &it.kind {
        ItemKind::Fn(f) => {
            if f.generics.is_empty() {
                let sig = cx.symbol_of(it).and_then(|s| match cx.table.get(s) {
                    Some(ItemSig::Fn(sig)) => Some(sig.clone()),
                    _ => None,
                });
                if let (Some(sig), Some(sym)) = (sig, cx.symbol_of(it)) {
                    let mut body =
                        Body::new(cx, None, sig.ret.clone(), BodyOwner::Fn(sym), f.name.span);
                    let root = body.function(f, &sig);
                    body.finish(root);
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
    check_owns_are_released(cx, sym);
    // Constructors are addressed by position in declaration order (spec §7.2).
    let mut ctors = 0usize;
    for m in members {
        match &m.kind {
            MemberKind::Field(f) => {
                if let Some(default) = &f.default {
                    let found = cx
                        .table
                        .struct_sig(sym)
                        .and_then(|s| s.fields.iter().position(|x| x.name == f.name.name));
                    let expected = found
                        .and_then(|i| cx.table.struct_sig(sym).map(|s| s.fields[i].ty.clone()))
                        .unwrap_or(Ty::Error);
                    let owner = BodyOwner::FieldDefault {
                        owner: sym,
                        index: found.unwrap_or(0),
                    };
                    let mut body = Body::new(cx, Some(sym), Ty::Void, owner, default.span);
                    let checked = body.expr(default, Some(&expected));
                    body.expect(&expected, &checked.ty, default.span);
                    body.finish_value(default.span, checked);
                }
            }
            MemberKind::Fn(f) => {
                if !f.generics.is_empty() {
                    continue;
                }
                let sig = cx
                    .table
                    .struct_sig(sym)
                    .and_then(|s| s.methods.get(&f.name.name))
                    .cloned();
                if let Some(sig) = sig {
                    let owner = BodyOwner::Method {
                        owner: sym,
                        name: f.name.name.clone(),
                    };
                    let mut body = Body::new(cx, Some(sym), sig.ret.clone(), owner, f.name.span);
                    let root = body.function(f, &sig);
                    body.finish(root);
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
                    fallible: false,
                    takes_self: Some(true),
                    is_mut: true,
                    vis: m.vis,
                    span: f.name.span,
                    generic: false,
                };
                // §6 gives `copy` and `drop` meaning no ordinary member has, so they are
                // addressable in the HIR by what they are rather than by name.
                let owner = match &m.kind {
                    MemberKind::Copy(_) => BodyOwner::Copy(sym),
                    MemberKind::Drop(_) => BodyOwner::Drop(sym),
                    _ => {
                        let index = ctors;
                        ctors += 1;
                        BodyOwner::Ctor { owner: sym, index }
                    }
                };
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
            .find(|f| matches!(f.ty, Ty::OwnPtr(_)))
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
    slot: Slot,
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
    self_mutable: bool,
    /// The `self` parameter's local, for expanding unqualified field access.
    self_local: Option<LocalId>,
    hir: BodyBuilder,
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
            self_mutable: false,
            self_local: None,
            hir,
        }
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
            scope.insert(
                name.to_owned(),
                Local {
                    ty,
                    mutable,
                    slot: Slot::Local(id),
                },
            );
        }
        id
    }

    /// Bring a field of `self` into scope unqualified (spec §7.3).
    fn bind_self_field(&mut self, name: &str, ty: Ty, mutable: bool, index: usize) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(
                name.to_owned(),
                Local {
                    ty,
                    mutable,
                    slot: Slot::SelfField { index },
                },
            );
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
            Ty::Ref { mutable, inner } => Ty::Ref {
                mutable: *mutable,
                inner: Rc::new(self.subst(inner)),
            },
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
        if let (Ty::Ref { inner: want, .. }, Ty::Ptr(got) | Ty::OwnPtr(got)) =
            (&expected, &actual)
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
        // Signatures are collected before any `Self` is known, so `copy(other: &Self)`
        // arrives here still spelled `Self`. Substituting at the binding is what makes
        // `other.n` resolve to the enclosing struct's field.
        self.ret = self.subst(&self.ret.clone());
        self.push();
        if let Some(is_mut) = sig.takes_self {
            self.self_mutable = is_mut;
            if let Some(sym) = self.self_ty {
                let id = self.bind(
                    "self",
                    Ty::Named(sym),
                    is_mut,
                    LocalKind::Param,
                    f.name.span,
                );
                self.self_local = Some(id);
                // A method body sees its struct's fields unqualified (spec §7.3). They are
                // fields, not locals, so they bind to a slot that remembers the index.
                let fields: Vec<(String, Ty)> = self
                    .cx
                    .table
                    .struct_sig(sym)
                    .map(|s| {
                        s.fields
                            .iter()
                            .map(|x| (x.name.clone(), x.ty.clone()))
                            .collect()
                    })
                    .unwrap_or_default();
                for (index, (name, ty)) in fields.into_iter().enumerate() {
                    self.bind_self_field(&name, ty, is_mut, index);
                }
            }
        }
        for (p, ps) in f.params.iter().zip(&sig.params) {
            let ty = self.subst(&ps.ty);
            let default = p.default.as_ref().map(|default| {
                let c = self.expr(default, Some(&ty));
                self.expect(&ty, &c.ty, default.span);
                c.id
            });
            self.hir.param_default(default);
            // Parameters are immutable bindings; mutate through `&mut` or `^`.
            self.bind(&ps.name, ty, false, LocalKind::Param, p.name.span);
        }
        let root = match &f.body {
            Some(body) => self.block(body, None).id,
            None => self.hir.block(f.name.span, Vec::new()),
        };
        self.pop();
        root
    }

    // --- statements -------------------------------------------------------

    /// The type a block produces: its final expression statement, or `void`.
    fn block(&mut self, b: &Block, expected: Option<&Ty>) -> CheckedBlock {
        self.push();
        let mut result = Ty::Void;
        let mut stmts = Vec::new();
        let last = b.stmts.len().saturating_sub(1);
        for (i, s) in b.stmts.iter().enumerate() {
            if i == last {
                if let StmtKind::Expr(e) = &s.kind {
                    // The final expression is the block's value only when something wants
                    // one; otherwise it is an ordinary statement, and `or ignore` is legal.
                    self.stmt_position = expected.is_none();
                    let c = self.expr(e, expected);
                    self.stmt_position = false;
                    self.check_unused(&c.ty, e);
                    result = c.ty;
                    stmts.push(self.hir.stmt(s.span, hir::StmtKind::Expr(c.id)));
                    continue;
                }
            }
            let id = self.stmt(s);
            stmts.push(id);
        }
        self.pop();
        CheckedBlock {
            ty: result,
            id: self.hir.block(b.span, stmts),
        }
    }

    fn stmt(&mut self, s: &Stmt) -> StmtId {
        let kind = match &s.kind {
            StmtKind::Let(l) => {
                let declared = l.ty.as_ref().map(|t| {
                    let t = self.cx.lower_type(t);
                    self.subst(&t)
                });
                let (ty, init) = match (&declared, &l.init) {
                    (Some(d), Some(init)) => {
                        let c = self.expr(init, Some(d));
                        self.expect(d, &c.ty, init.span);
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
                    _ => hir::StmtKind::Let {
                        local: locals.into_iter().next().flatten(),
                        init,
                    },
                }
            }
            StmtKind::Assign { target, op, value } => {
                let t = self.expr(target, None);
                let v = self.expr(value, Some(&t.ty));
                self.require_mutable(target);
                if *op == AssignOp::Assign {
                    self.expect(&t.ty, &v.ty, value.span);
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
                hir::StmtKind::Assign {
                    target: t.id,
                    op: *op,
                    value: v.id,
                }
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
                let body = self.block(&w.body, None).id;
                hir::StmtKind::While { cond: c.id, body }
            }
            StmtKind::For(f) => {
                let subject = self.expr(&f.iter, None);
                let elem = match subject.ty.element() {
                    Some(e) => e.clone(),
                    None => {
                        if !subject.ty.is_wildcard() {
                            let t = self.cx.name_of(&subject.ty);
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
                let mut bindings = Vec::new();
                if f.bindings.len() == 2 {
                    let idx = Ty::Int(IntTy::Usize);
                    bindings.extend(self.bind_pattern(
                        &f.bindings[0],
                        &idx,
                        false,
                        LocalKind::Binding,
                    ));
                    bindings.extend(self.bind_pattern(
                        &f.bindings[1],
                        &elem,
                        false,
                        LocalKind::Binding,
                    ));
                } else {
                    for b in &f.bindings {
                        bindings.extend(self.bind_pattern(b, &elem, false, LocalKind::Binding));
                    }
                }
                let body = self.block(&f.body, None).id;
                self.pop();
                hir::StmtKind::For {
                    bindings,
                    subject: subject.id,
                    body,
                }
            }
            StmtKind::Defer(inner) => hir::StmtKind::Defer(self.stmt(inner)),
            // Nested declarations are checked by the top-level walk.
            StmtKind::Item(_) | StmtKind::Error => hir::StmtKind::Error,
        };
        self.hir.stmt(s.span, kind)
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
        // An `or` clause has already consumed it.
        if matches!(e.kind, ExprKind::Or { .. }) {
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
                        .at(
                            self.cx.file,
                            e.span,
                            "this may be absent, and that is discarded",
                        ),
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
                    // A slice falls through to its binding, so `xs[i] = v` on an
                    // `xs: []T` parameter is rejected. A slice is a pointer and a length,
                    // and indexing one arguably ought to be mutable the way `^T` is —
                    // but that is a language question (§5.3). Owed by M6.
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
                    (
                        format!("`{name}`"),
                        "constants and functions cannot be assigned to".to_owned(),
                    )
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

    /// Type of an expression without emitting diagnostics or keeping nodes, for place
    /// analysis. The expression has already been checked once; this is a second look.
    fn expr_quiet(&mut self, e: &Expr) -> Ty {
        let before = self.cx.diags.len();
        let mark = self.hir.mark();
        let t = self.expr(e, None).ty;
        self.hir.rollback(mark);
        self.cx.diags.truncate(before);
        t
    }

    // --- expressions ------------------------------------------------------

    fn expr(&mut self, e: &Expr, expected: Option<&Ty>) -> Checked {
        match &e.kind {
            ExprKind::Int(text) => {
                let value = parse_int_literal(text).unwrap_or(0);
                let mut ty = Ty::IntLit;
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
                            ty = Ty::Error;
                        } else {
                            ty = Ty::Int(i);
                        }
                    } else if let Ty::Float(f) = self.subst(exp) {
                        // An integer literal in a float context is a float (spec §2.5).
                        return self.mk(e, Ty::Float(f), hir::ExprKind::Float(value as f64));
                    }
                }
                self.mk(e, ty, hir::ExprKind::Int(value))
            }
            ExprKind::Float(text) => {
                let ty = match expected.map(|t| self.subst(t)) {
                    Some(Ty::Float(f)) => Ty::Float(f),
                    _ => Ty::FloatLit,
                };
                self.mk(e, ty, hir::ExprKind::Float(parse_float_literal(text)))
            }
            ExprKind::Str(text) => {
                let kind = hir::ExprKind::Str(decode_string_literal(text));
                self.mk(e, Ty::Str, kind)
            }
            ExprKind::RawStr(text) => {
                let kind = hir::ExprKind::Str(decode_raw_string_literal(text));
                self.mk(e, Ty::Str, kind)
            }
            ExprKind::Char(text) => {
                let c = decode_string_literal(text).chars().next().unwrap_or('\0');
                self.mk(e, Ty::Char, hir::ExprKind::Char(c))
            }
            ExprKind::Bool(b) => self.mk(e, Ty::Bool, hir::ExprKind::Bool(*b)),
            ExprKind::Null => {
                let ty = match expected.map(|t| self.subst(t)) {
                    Some(t @ (Ty::Ptr(_) | Ty::OwnPtr(_))) => t,
                    _ => Ty::Ptr(Rc::new(Ty::Error)),
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
            ExprKind::Path(p) => self.path_expr(p, e),
            ExprKind::Tuple(elems) => {
                let checked: Vec<Checked> = match expected.map(|t| self.subst(t)) {
                    Some(Ty::Tuple(want)) if want.len() == elems.len() => elems
                        .iter()
                        .zip(&want)
                        .map(|(x, w)| self.expr(x, Some(w)))
                        .collect(),
                    _ => elems.iter().map(|x| self.expr(x, None)).collect(),
                };
                let ty = Ty::Tuple(checked.iter().map(|c| c.ty.defaulted()).collect());
                let parts = checked.iter().map(|c| c.id).collect();
                self.mk(e, ty, hir::ExprKind::Tuple(parts))
            }
            ExprKind::Ref { mutable, expr } => {
                let inner = self.expr(expr, None);
                if *mutable {
                    self.require_mutable(expr);
                }
                let ty = Ty::Ref {
                    mutable: *mutable,
                    inner: Rc::new(inner.ty.defaulted()),
                };
                self.mk(
                    e,
                    ty,
                    hir::ExprKind::Ref {
                        mutable: *mutable,
                        place: inner.id,
                    },
                )
            }
            ExprKind::Deref(inner) => {
                let c = self.expr(inner, None);
                let ty = match &c.ty {
                    Ty::Ptr(i) | Ty::OwnPtr(i) => (**i).clone(),
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
                let c = self.expr(operand, expected);
                let t = c.ty.clone();
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
                self.mk(
                    e,
                    ty,
                    hir::ExprKind::Unary {
                        op: *op,
                        operand: c.id,
                    },
                )
            }
            ExprKind::Binary { op, lhs, rhs } => self.binary(*op, lhs, rhs, e, expected),
            ExprKind::Cast { expr, ty } => {
                let from = self.expr(expr, None);
                let target = self.cx.lower_type(ty);
                let target = self.subst(&target);
                self.check_cast(&from.ty, &target, e.span);
                self.mk(e, target, hir::ExprKind::Cast { operand: from.id })
            }
            ExprKind::Range {
                start,
                end,
                inclusive,
            } => {
                let start = start.as_ref().map(|x| self.expr(x, None));
                let end = end.as_ref().map(|x| self.expr(x, None));
                let (s, en) = (
                    start.as_ref().map(|c| c.ty.clone()),
                    end.as_ref().map(|c| c.ty.clone()),
                );
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
            ExprKind::Index { base, index } => {
                let b = self.expr(base, None);
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
                let ty = match b.ty.element() {
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
                self.mk(
                    e,
                    ty,
                    hir::ExprKind::Index {
                        base: b.id,
                        index: i.id,
                    },
                )
            }
            ExprKind::Field { base, name } => {
                let b = self.expr(base, None);
                let (ty, index) = self.field_type(&b.ty, name, base.span);
                match index {
                    // Tuples are indexed by position, structs by field (spec §5.4).
                    Some(index) if matches!(b.ty.peeled(), Ty::Tuple(_)) => {
                        self.mk(e, ty, hir::ExprKind::TupleIndex { base: b.id, index })
                    }
                    Some(index) => self.mk(e, ty, hir::ExprKind::Field { base: b.id, index }),
                    None => self.mk(e, ty, hir::ExprKind::Error),
                }
            }
            ExprKind::Call { callee, args } => self.call(callee, args, e),
            ExprKind::StructLit { path, fields } => {
                // `Shape::Circle{ … }` is a union value, not a struct: the path names a
                // variant, and `Shape::Circle` is not a type of its own (spec §7.5).
                if let Some((owner, index)) = self.union_variant_path(path) {
                    let kind = self.union_variant_literal(owner, index, fields, e.span);
                    return self.mk(e, Ty::Named(owner), kind);
                }
                let ty = self.cx.lower_type(&Type {
                    span: path.span,
                    kind: TypeKind::Path(path.clone()),
                });
                let ty = self.subst(&ty);
                let kind = self.struct_literal(&ty, fields, e.span);
                self.mk(e, ty, kind)
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
                    return self.mk(e, Ty::Error, hir::ExprKind::Error);
                };
                let kind = self.dot_infer(&want, name, init.as_deref(), e.span);
                self.mk(e, want, kind)
            }
            ExprKind::Block(b) => {
                let c = self.block(b, expected);
                self.mk(e, c.ty, hir::ExprKind::Block(c.id))
            }
            ExprKind::If(i) => {
                let cond = self.expr(&i.cond, Some(&Ty::Bool));
                self.require_bool(&cond.ty, i.cond.span, "if");
                let then = self.block(&i.then, expected);
                let (ty, else_branch) = match &i.else_branch {
                    Some(else_expr) => {
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
                let kind = hir::ExprKind::If {
                    cond: cond.id,
                    then: then.id,
                    else_branch,
                };
                self.mk(e, ty, kind)
            }
            ExprKind::Switch(s) => self.switch(s, e, expected),
            ExprKind::Or { expr, clause } => self.or_expr(expr, clause, e, expected),
            ExprKind::Return(v) => {
                let ret = self.ret.clone();
                let mut value = None;
                match v {
                    Some(expr) => {
                        let c = self.expr(expr, Some(&ret));
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
                                    "mark the signature fallible by writing `!->` in place                                      of `->` (spec §9.1)",
                                );
                            }
                        } else {
                            // Returning a plain `T` from a `T?` function is the success
                            // case — there is no wrapping step (spec §9.1).
                            let target = match &ret {
                                Ty::Optional(inner) if !assignable(&ret, &t) => {
                                    (**inner).clone()
                                }
                                other => other.clone(),
                            };
                            self.expect(&target, &t, expr.span);
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
            ExprKind::Break { value, .. } => {
                let value = value.as_ref().map(|v| self.expr(v, None).id);
                self.mk(e, Ty::Never, hir::ExprKind::Break { value })
            }
            ExprKind::Continue { .. } => self.mk(e, Ty::Never, hir::ExprKind::Continue),
            ExprKind::Intrinsic { name, args } => {
                let args = args.iter().map(|a| self.expr(a, None).id).collect();
                // spec §12.7
                let ty = match name.name.as_str() {
                    "line" | "column" => Ty::Int(IntTy::U32),
                    "file" | "function" | "module" | "expr_text" => Ty::Str,
                    _ => Ty::Error,
                };
                let kind = hir::ExprKind::Intrinsic {
                    name: name.name.clone(),
                    args,
                };
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
                let a = self.expr(lhs, None);
                let b = self.expr(rhs, Some(&a.ty));
                if join(&a.ty, &b.ty).is_none() {
                    let (x, y) = (self.cx.name_of(&a.ty), self.cx.name_of(&b.ty));
                    self.cx.error_help(
                        codes::INVALID_OPERANDS,
                        span,
                        format!("cannot compare `{x}` with `{y}`"),
                        "operands have different types",
                        "Dazzle has no implicit conversions (spec §5.1) — cast one side",
                    );
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
        self.mk(
            e,
            ty,
            hir::ExprKind::Binary {
                op,
                lhs: a.id,
                rhs: b.id,
            },
        )
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
        // Slices carry a length (spec §5.3).
        if let Ty::Array { .. } = &peeled {
            if name.name == "len" {
                return (Ty::Int(IntTy::Usize), None);
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
        if let Ty::Variant(owner, index) = &peeled {
            let variant = match self.cx.table.get(*owner) {
                Some(ItemSig::Union { variants }) => variants.get(*index).cloned(),
                _ => None,
            };
            let Some(variant) = variant else { return (Ty::Error, None) };
            let found = variant
                .fields
                .iter()
                .enumerate()
                .find(|(_, f)| f.name == name.name);
            return match found {
                Some((i, f)) => (f.ty.clone(), Some(i)),
                None => {
                    let owner_name = self.cx.res.symbol(*owner).name.clone();
                    let names: Vec<String> =
                        variant.fields.iter().map(|f| f.name.clone()).collect();
                    let suggestion =
                        dz_resolve::closest(&name.name, names.iter().map(String::as_str));
                    let mut d = dz_diag::Diagnostic::error(
                        codes::NO_SUCH_FIELD,
                        format!(
                            "`{owner_name}::{}` has no field `{}`",
                            variant.name, name.name
                        ),
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
        if self.cx.table.is_generic(sym) {
            return (Ty::Error, None); // generic members wait for M6
        }
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
        let found = sig
            .fields
            .iter()
            .enumerate()
            .find(|(_, f)| f.name == name.name);
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
        (field.ty, Some(index))
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
            .with_label(dz_diag::Label::secondary(
                decl_file,
                decl_span,
                "declared here",
            ))
            .with_help(help),
        );
    }

    fn call(&mut self, callee: &Expr, args: &[Expr], e: &Expr) -> Checked {
        let span = e.span;
        // A method call parses as a call on a field access.
        if let ExprKind::Field { base, name } = &callee.kind {
            let recv = self.expr(base, None);
            let base_ty = recv.ty.clone();
            let peeled = base_ty.peeled().clone();
            if let Some(sym) = peeled.named() {
                if !self.cx.table.is_generic(sym) {
                    if let Some(sig) = self
                        .cx
                        .table
                        .struct_sig(sym)
                        .and_then(|s| s.methods.get(&name.name))
                        .cloned()
                    {
                        self.check_member_visible(
                            sym, &name.name, sig.vis, sig.span, name.span, "method",
                        );
                        if sig.is_mut {
                            self.require_mutable_receiver(base, &base_ty, &name.name);
                        }
                        let args = self.check_args(&sig, args, span, &name.name);
                        let ty = fallible_result(&sig, self.subst(&sig.ret));
                        let callee = hir::Callee::Method {
                            recv: recv.id,
                            owner: sym,
                            name: name.name.clone(),
                        };
                        return self.mk(e, ty, hir::ExprKind::Call { callee, args });
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
                        return self.mk(e, Ty::Error, hir::ExprKind::Error);
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
                    let ctors = self
                        .cx
                        .table
                        .struct_sig(sym)
                        .map(|s| s.ctors.clone())
                        .unwrap_or_default();
                    if let Some(sig) = ctors.first().cloned() {
                        let name = self.cx.res.symbol(sym).name.clone();
                        let args = self.check_args(&sig, args, span, &name);
                        let callee = hir::Callee::Ctor {
                            owner: sym,
                            index: 0,
                        };
                        return self.mk(e, Ty::Named(sym), hir::ExprKind::Call { callee, args });
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
                        let args = self.check_args(&sig, args, span, &name);
                        let ty = fallible_result(&sig, self.subst(&sig.ret));
                        let callee = hir::Callee::Fn(sym);
                        return self.mk(e, ty, hir::ExprKind::Call { callee, args });
                    }
                }
            }

            // Builtins are variadic and untyped until the stdlib exists. §12.9's
            // format-string checking is owed by M6 — see the deferral register.
            if p.segments.len() == 1
                && dz_resolve::BUILTIN_FNS.contains(&p.segments[0].name.name.as_str())
            {
                let name = p.segments[0].name.name.clone();
                let args = args.iter().map(|a| self.expr(a, None).id).collect();
                let ty = builtin_call_type(&name);
                return self.mk(e, ty, hir::ExprKind::Builtin { name, args });
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
        self.mk(
            e,
            ty,
            hir::ExprKind::Call {
                callee,
                args: arg_ids,
            },
        )
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

    /// Check a call's arguments against a signature, returning the nodes built for them.
    fn check_args(
        &mut self,
        sig: &FnSig,
        args: &[Expr],
        span: Span,
        name: &str,
    ) -> Vec<hir::ExprId> {
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

    /// The declaration a path's *owner* segment names: the item itself for `f`, and the
    /// type or module member for `Type::Variant` and `module::f`.
    fn path_symbol(&self, p: &Path) -> Option<SymbolId> {
        let first = p.segments.first()?;
        if p.segments.len() > 1 {
            if let Some(target) = self.cx.res.module_alias(self.cx.file, &first.name.name) {
                return self.cx.res.lookup(target, &p.segments.get(1)?.name.name);
            }
        }
        // `Layer::Ui` — the leading segment is a type in scope, not a module. Without
        // this the path resolved to nothing and, because "nothing" types as `Ty::Error`,
        // it was accepted in silence rather than reported (spec §3.3).
        self.cx
            .res
            .lookup_in_file(self.cx.module, self.cx.file, &first.name.name)
    }

    fn path_expr(&mut self, p: &Path, e: &Expr) -> Checked {
        let Some(first) = p.segments.first() else {
            return self.mk(e, Ty::Error, hir::ExprKind::Error);
        };

        // A local wins over everything for a bare name.
        if p.segments.len() == 1 {
            if let Some(l) = self.local(&first.name.name).cloned() {
                let kind = match l.slot {
                    Slot::Local(id) => hir::ExprKind::Local(id),
                    // An unqualified field in a method body is `self.field` (spec §7.3).
                    Slot::SelfField { index } => match self.self_local {
                        Some(base) => {
                            let self_ty = self.self_ty.map(Ty::Named).unwrap_or(Ty::Error);
                            let base = self.hir.expr(e.span, self_ty, hir::ExprKind::Local(base));
                            hir::ExprKind::Field {
                                base: base.id,
                                index,
                            }
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
        }

        let Some(sym) = self.path_symbol(p) else {
            return self.mk(e, Ty::Error, hir::ExprKind::Error);
        };
        let symbol_kind = self.cx.res.symbol(sym).kind;

        // `Type::Variant` and `module::Type::Variant`
        let tail = if p.segments.len() > 1
            && self
                .cx
                .res
                .module_alias(self.cx.file, &first.name.name)
                .is_some()
        {
            &p.segments[2..]
        } else {
            &p.segments[1..]
        };

        if let Some(seg) = tail.first() {
            let (ty, kind) = self.assoc_item(sym, seg);
            return self.mk(e, ty, kind);
        }

        let (ty, kind) = match symbol_kind {
            SymbolKind::Fn => match self.cx.table.get(sym) {
                Some(ItemSig::Fn(sig)) => (
                    Ty::Fn {
                        params: sig.params.iter().map(|p| p.ty.clone()).collect(),
                        ret: Rc::new(sig.ret.clone()),
                    },
                    hir::ExprKind::FnItem(sym),
                ),
                _ => (Ty::Error, hir::ExprKind::Error),
            },
            SymbolKind::Const => match self.cx.table.get(sym) {
                Some(ItemSig::Const(t)) => (t.clone(), hir::ExprKind::Const(sym)),
                _ => (Ty::Error, hir::ExprKind::Error),
            },
            // A bare type name in value position is only meaningful as a constructor
            // call, which `call` handles before reaching here.
            _ => (Ty::Named(sym), hir::ExprKind::Error),
        };
        self.mk(e, ty, kind)
    }

    fn assoc_item(&mut self, owner: SymbolId, seg: &PathSegment) -> (Ty, hir::ExprKind) {
        match self.cx.table.get(owner) {
            Some(ItemSig::Enum { variants, .. }) => {
                match variants.iter().position(|v| v.name == seg.name.name) {
                    Some(index) => (
                        Ty::Named(owner),
                        hir::ExprKind::EnumVariant { owner, index },
                    ),
                    // Name resolution already reported the unknown variant.
                    None => (Ty::Error, hir::ExprKind::Error),
                }
            }
            Some(ItemSig::Union { variants }) => {
                match variants.iter().position(|v| v.name == seg.name.name) {
                    Some(index) => (
                        Ty::Named(owner),
                        hir::ExprKind::UnionVariant {
                            owner,
                            index,
                            payload: Vec::new(),
                        },
                    ),
                    None => (Ty::Error, hir::ExprKind::Error),
                }
            }
            _ => (Ty::Error, hir::ExprKind::Error),
        }
    }

    /// Check a struct literal and build its node. Values come back in *declaration*
    /// order, with `None` where the field's declared default applies, so no consumer has
    /// to consult the declaration to know what order to emit initialisers in.
    fn struct_literal(&mut self, ty: &Ty, fields: &[FieldInit], span: Span) -> hir::ExprKind {
        let Some(sym) = ty.named() else {
            for f in fields {
                self.expr(&f.value, None);
            }
            return hir::ExprKind::Error;
        };
        if self.cx.table.is_generic(sym) {
            for f in fields {
                self.expr(&f.value, None);
            }
            return hir::ExprKind::Error;
        }
        let Some(sig) = self.cx.table.struct_sig(sym).cloned() else {
            for f in fields {
                self.expr(&f.value, None);
            }
            return hir::ExprKind::Error;
        };
        let owner = self.cx.res.symbol(sym).name.clone();
        let values = self.field_values(&sig.fields, Some(sym), &owner, fields, span);
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
                self.check_member_visible(
                    sym, &decl_name, decl_vis, decl_span, name.span, "field",
                );
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
                    if missing.len() == 1 {
                        "field"
                    } else {
                        "fields"
                    }
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
        index: usize,
        fields: &[FieldInit],
        span: Span,
    ) -> hir::ExprKind {
        let variant = match self.cx.table.get(owner) {
            Some(ItemSig::Union { variants }) => variants.get(index).cloned(),
            _ => None,
        };
        let Some(variant) = variant else { return hir::ExprKind::Error };
        let name = format!(
            "{}::{}",
            self.cx.res.symbol(owner).name,
            variant.name
        );
        // Visibility is the union's, not a per-field thing: a variant's fields are as
        // public as the variant is, so nothing is checked per field here.
        let payload = self.field_values(&variant.fields, None, &name, fields, span);
        hir::ExprKind::UnionVariant { owner, index, payload }
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

    fn dot_infer(
        &mut self,
        want: &Ty,
        name: &Ident,
        init: Option<&[FieldInit]>,
        span: Span,
    ) -> hir::ExprKind {
        let Some(sym) = want.named() else {
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
            Some(index) => self.union_variant_literal(sym, index, init.unwrap_or(&[]), span),
            None => {
                for f in init.unwrap_or(&[]) {
                    self.expr(&f.value, None);
                }
                hir::ExprKind::Error
            }
        }
    }

    fn switch(&mut self, s: &SwitchExpr, e: &Expr, expected: Option<&Ty>) -> Checked {
        let subject = self.expr(&s.subject, None);
        let peeled = subject.ty.peeled().clone();
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
        let mut hir_arms: Vec<hir::Arm> = Vec::new();

        for arm in &s.arms {
            self.push();
            let mut indices = Vec::new();
            let mut binding = None;
            for pat in &arm.patterns {
                let known = variants.iter().find(|v| v.name == pat.name.name).cloned();
                match &known {
                    Some(v) => {
                        covered.push(v.name.clone());
                        if let Some(i) = variants.iter().position(|x| x.name == v.name) {
                            indices.push(i);
                        }
                        if let Some(b) = &pat.binding {
                            // The binding is a reference into the union's payload, so
                            // matching never copies (spec §10.3). Its type is the
                            // variant's field list, not the union — without that,
                            // `c.radius` had nothing to resolve against.
                            let index = variants.iter().position(|x| x.name == v.name);
                            let payload = match (is_union, sym, index) {
                                (true, Some(owner), Some(i)) => Ty::Variant(owner, i),
                                _ => Ty::Error,
                            };
                            binding =
                                Some(self.bind(&b.name, payload, true, LocalKind::Binding, b.span));
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
            let body = self.block(&arm.body, expected);
            result = Some(match result {
                Some(prev) => join(&prev, &body.ty).unwrap_or(Ty::Void),
                None => body.ty.clone(),
            });
            hir_arms.push(hir::Arm {
                span: arm.span,
                variants: indices,
                binding,
                body: body.id,
            });
            self.pop();
        }

        let mut default = None;
        if let Some(d) = &s.default {
            let c = self.block(d, expected);
            default = Some(c.id);
            result = Some(match result {
                Some(prev) => join(&prev, &c.ty).unwrap_or(Ty::Void),
                None => c.ty,
            });
        } else if !variants.is_empty() {
            let missing: Vec<String> = variants
                .iter()
                .filter(|v| !covered.contains(&v.name))
                .map(|v| format!("`{}`", v.name))
                .collect();
            if !missing.is_empty() {
                let owner = sym
                    .map(|id| self.cx.res.symbol(id).name.clone())
                    .unwrap_or_default();
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

        let ty = result.unwrap_or(Ty::Void);
        let kind = hir::ExprKind::Switch {
            subject: subject.id,
            arms: hir_arms,
            default,
        };
        self.mk(e, ty, kind)
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
        let (success, payload) = match &subject.ty {
            // A fallible call is the only shape that carries an `Error` to bind.
            Ty::Fallible(inner) => (
                (**inner).clone(),
                Some(Ty::Builtin(crate::ty::BuiltinTy::Error)),
            ),
            Ty::Optional(inner) => ((**inner).clone(), None),
            // `^T or …` is the checked pointer-to-reference conversion of §5.2: the
            // failure case is null, and the success case is the reference it becomes.
            Ty::Ptr(inner) | Ty::OwnPtr(inner) => {
                (Ty::Ref { mutable: false, inner: inner.clone() }, None)
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
                if !carries {
                    let ret = self.cx.name_of(&self.ret.clone());
                    let help = if matches!(subject.ty, Ty::Fallible(_)) {
                        "mark this function `!->`, or handle the failure here with a                          fallback or a block (spec §9.5)"
                            .to_owned()
                    } else {
                        format!(
                            "this function returns `{ret}`, so a bare `return` has no                              value to give — supply a fallback instead (spec §9.5)"
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
                    hir::ExprKind::Or {
                        operand: subject.id,
                        clause: hir::OrClause::Value(c),
                    },
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
