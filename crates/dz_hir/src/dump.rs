//! A span-free textual dump of the HIR, in the style of `dz_ast::dump`.
//!
//! This is what makes the tree reviewable. An IR that nothing prints is an IR whose bugs
//! only ever surface as wrong C++ three phases later, so every body gets a form a test can
//! assert on and a person can read. Types are shown on the nodes that carry them, because
//! "which type did the checker actually conclude here" is the question this dump exists to
//! answer.
//!
//! Span-free on purpose, like the AST dump: the shape and the types are the contract, and
//! byte offsets move whenever anything above them in the file changes.

use std::fmt::Write;

use crate::ty::Ty;
use crate::*;

/// Names for the symbols a body mentions. The HIR stores [`SymbolId`]s, which are indices
/// with no text of their own, so the caller supplies the lookup.
pub trait Names {
    fn name_of_symbol(&self, id: SymbolId) -> String;
    fn name_of_type(&self, ty: &Ty) -> String;
}

pub fn dump(hir: &Hir, names: &dyn Names) -> String {
    let mut d = Dumper { out: String::new(), indent: 0, body: None, names };
    for (_, body) in hir.bodies() {
        d.body(body);
    }
    d.out
}

struct Dumper<'a> {
    out: String,
    indent: usize,
    body: Option<&'a Body>,
    names: &'a dyn Names,
}

impl<'a> Dumper<'a> {
    /// `<f32, bool>` for a generic method's solved arguments; nothing when there are none.
    fn type_list(&self, tys: &[Ty]) -> String {
        if tys.is_empty() {
            return String::new();
        }
        let names: Vec<String> = tys.iter().map(|t| self.names.name_of_type(t)).collect();
        format!("<{}>", names.join(", "))
    }

    fn line(&mut self, args: std::fmt::Arguments<'_>) {
        for _ in 0..self.indent {
            self.out.push_str("  ");
        }
        let _ = self.out.write_fmt(args);
        self.out.push('\n');
    }

    fn nest(&mut self, f: impl FnOnce(&mut Self)) {
        self.indent += 1;
        f(self);
        self.indent -= 1;
    }

    fn body(&mut self, body: &'a Body) {
        let owner = match &body.owner {
            BodyOwner::Fn(s) => format!("fn {}", self.names.name_of_symbol(*s)),
            BodyOwner::Method { owner, name } => {
                format!("method {}::{name}", self.names.name_of_symbol(*owner))
            }
            BodyOwner::Ctor { owner, index } => {
                format!("ctor {}#{index}", self.names.name_of_symbol(*owner))
            }
            BodyOwner::Copy(s) => format!("copy {}", self.names.name_of_symbol(*s)),
            BodyOwner::Drop(s) => format!("drop {}", self.names.name_of_symbol(*s)),
            BodyOwner::Const(s) => format!("const {}", self.names.name_of_symbol(*s)),
            BodyOwner::Static(s) => format!("static {}", self.names.name_of_symbol(*s)),
            BodyOwner::AssocConst { owner, name } => {
                format!("const {}::{name}", self.names.name_of_symbol(*owner))
            }
            BodyOwner::FieldDefault { owner, index } => {
                format!("field-default {}.{index}", self.names.name_of_symbol(*owner))
            }
            BodyOwner::ImplMethod { imp, name } => format!("impl#{imp} method {name}"),
            BodyOwner::TraitDefaults { owner, name } => {
                format!("defaults {}::{name}", self.names.name_of_symbol(*owner))
            }
        };
        let ret = self.names.name_of_type(&body.ret);
        let arrow = if body.fallible { "!->" } else { "->" };
        self.line(format_args!("body {owner} {arrow} {ret}"));
        self.body = Some(body);
        self.nest(|d| {
            for local in &body.params {
                let l = body.local(*local);
                let t = d.names.name_of_type(&l.ty);
                d.line(format_args!("param _{} {}: {t}", local.0, l.name));
            }
            d.block(body.root);
        });
        self.body = None;
    }

    fn block(&mut self, id: BlockId) {
        let Some(body) = self.body else { return };
        let stmts = body.block(id).stmts.clone();
        self.line(format_args!("block"));
        self.nest(|d| {
            if stmts.is_empty() {
                d.line(format_args!("(empty)"));
            }
            for s in stmts {
                d.stmt(s);
            }
        });
    }

    fn local_name(&self, id: LocalId) -> String {
        match self.body {
            Some(b) => format!("_{} {}", id.0, b.local(id).name),
            None => format!("_{}", id.0),
        }
    }

    fn opt_local(&self, id: &Option<LocalId>) -> String {
        match id {
            Some(id) => self.local_name(*id),
            None => "_".to_owned(),
        }
    }

    fn stmt(&mut self, id: StmtId) {
        let Some(body) = self.body else { return };
        match body.stmt(id).kind.clone() {
            StmtKind::Let { local, init } => {
                let name = self.opt_local(&local);
                let ty = match local {
                    Some(l) => self.names.name_of_type(&body.local(l).ty),
                    None => "_".to_owned(),
                };
                self.line(format_args!("let {name}: {ty}"));
                self.nest(|d| match init {
                    Some(e) => d.expr(e),
                    None => d.line(format_args!("(uninitialised)")),
                });
            }
            StmtKind::LetTuple { locals, init } => {
                let names: Vec<String> = locals.iter().map(|l| self.opt_local(l)).collect();
                self.line(format_args!("let ({})", names.join(", ")));
                self.nest(|d| d.expr(init));
            }
            StmtKind::Assign { target, op, value } => {
                self.line(format_args!("assign {}", op.as_str()));
                self.nest(|d| {
                    d.expr(target);
                    d.expr(value);
                });
            }
            StmtKind::Expr(e) => self.expr(e),
            StmtKind::While { cond, body: b, label } => {
                self.line(format_args!("while{}", label_str(&label)));
                self.nest(|d| {
                    d.expr(cond);
                    d.block(b);
                });
            }
            StmtKind::For { bindings, subject, body: b, shape, label } => {
                let names: Vec<String> = bindings.iter().map(|l| self.opt_local(l)).collect();
                let shape = match shape {
                    ForShape::Single => "",
                    ForShape::Tuple => " (tuple)",
                    ForShape::Indexed => " (indexed)",
                    ForShape::Consuming => " (consuming)",
                };
                self.line(format_args!("for {}{shape}{}", names.join(", "), label_str(&label)));
                self.nest(|d| {
                    d.expr(subject);
                    d.block(b);
                });
            }
            StmtKind::Defer(inner) => {
                self.line(format_args!("defer"));
                self.nest(|d| d.stmt(inner));
            }
            StmtKind::Item(s) => {
                let name = self.names.name_of_symbol(s);
                self.line(format_args!("item {name}"));
            }
            StmtKind::Error => self.line(format_args!("error")),
        }
    }

    fn expr(&mut self, id: ExprId) {
        let Some(body) = self.body else { return };
        let e = body.expr(id);
        let ty = self.names.name_of_type(&e.ty);
        let kind = e.kind.clone();
        macro_rules! node {
            ($($arg:tt)*) => {{
                let head = format!($($arg)*);
                self.line(format_args!("{head}: {ty}"));
            }};
        }
        match kind {
            ExprKind::Error => node!("error"),
            ExprKind::Int(v) => node!("int {v}"),
            ExprKind::Float(v) => node!("float {v}"),
            ExprKind::Bool(v) => node!("bool {v}"),
            ExprKind::Char(v) => node!("char {v:?}"),
            ExprKind::Str(v) => node!("str {v:?}"),
            ExprKind::Null => node!("null"),
            ExprKind::NoneOpt => node!("none"),
            ExprKind::Local(l) => {
                let name = self.local_name(l);
                node!("local {name}")
            }
            ExprKind::Const(s) => {
                let name = self.names.name_of_symbol(s);
                node!("const {name}")
            }
            ExprKind::Static(s) => {
                let name = self.names.name_of_symbol(s);
                node!("static {name}")
            }
            ExprKind::AssocConst { owner, name } => {
                let o = self.names.name_of_symbol(owner);
                node!("const {o}::{name}")
            }
            ExprKind::FnItem { sym: s, type_args } => {
                let name = self.names.name_of_symbol(s);
                let t = self.type_list(&type_args);
                node!("fn-item {name}{t}")
            }
            ExprKind::Field { base, index } => {
                node!("field #{index}");
                self.nest(|d| d.expr(base));
            }
            ExprKind::TupleIndex { base, index } => {
                node!("tuple-index #{index}");
                self.nest(|d| d.expr(base));
            }
            ExprKind::Index { base, index } => {
                node!("index");
                self.nest(|d| {
                    d.expr(base);
                    d.expr(index);
                });
            }
            ExprKind::SliceRange { base, start, end } => {
                node!("slice");
                self.nest(|d| {
                    d.expr(base);
                    for x in [start, end].into_iter().flatten() {
                        d.expr(x);
                    }
                });
            }
            ExprKind::Deref(inner) => {
                node!("deref");
                self.nest(|d| d.expr(inner));
            }
            ExprKind::Ref { mutable, place, .. } => {
                node!("ref{}", if mutable { " mut" } else { "" });
                self.nest(|d| d.expr(place));
            }
            ExprKind::Unary { op, operand } => {
                node!("unary {}", op.as_str());
                self.nest(|d| d.expr(operand));
            }
            ExprKind::Binary { op, lhs, rhs } => {
                node!("binary {}", op.as_str());
                self.nest(|d| {
                    d.expr(lhs);
                    d.expr(rhs);
                });
            }
            ExprKind::Cast { operand, .. } => {
                node!("cast");
                self.nest(|d| d.expr(operand));
            }
            ExprKind::Range { start, end, inclusive } => {
                node!("range{}", if inclusive { " inclusive" } else { "" });
                self.nest(|d| {
                    if let Some(s) = start {
                        d.expr(s);
                    }
                    if let Some(e) = end {
                        d.expr(e);
                    }
                });
            }
            ExprKind::StructLit { ty: sym, fields } => {
                let name = self.names.name_of_symbol(sym);
                node!("struct-lit {name}");
                self.nest(|d| {
                    for (i, f) in fields.iter().enumerate() {
                        match f {
                            Some(e) => {
                                d.line(format_args!("#{i}"));
                                d.nest(|d| d.expr(*e));
                            }
                            None => d.line(format_args!("#{i} (default)")),
                        }
                    }
                });
            }
            ExprKind::Tuple(parts) => {
                node!("tuple");
                self.nest(|d| {
                    for p in parts {
                        d.expr(p);
                    }
                });
            }
            ExprKind::ArrayLit(parts) => {
                node!("array");
                self.nest(|d| {
                    for p in parts {
                        d.expr(p);
                    }
                });
            }
            ExprKind::EnumVariant { owner, index } => {
                let name = self.names.name_of_symbol(owner);
                node!("enum-variant {name}#{index}")
            }
            ExprKind::UnionVariant { owner, index, payload } => {
                let name = self.names.name_of_symbol(owner);
                node!("union-variant {name}#{index}");
                self.nest(|d| {
                    for (i, f) in payload.iter().enumerate() {
                        match f {
                            Some(e) => {
                                d.line(format_args!("#{i}"));
                                d.nest(|d| d.expr(*e));
                            }
                            None => d.line(format_args!("#{i} (default)")),
                        }
                    }
                });
            }
            ExprKind::Call { callee, args } => {
                match &callee {
                    Callee::Fn { sym: s, .. } => {
                        let name = self.names.name_of_symbol(*s);
                        node!("call {name}")
                    }
                    Callee::Method { owner, name, method_args, .. } => {
                        let o = self.names.name_of_symbol(*owner);
                        let m = self.type_list(method_args);
                        node!("call-method {o}::{name}{m}")
                    }
                    Callee::BaseMethod { owner, name, .. } => {
                        let o = self.names.name_of_symbol(*owner);
                        node!("call-base-method {o}::{name}")
                    }
                    Callee::Assoc { owner, name, method_args, .. } => {
                        let o = self.names.name_of_symbol(*owner);
                        let m = self.type_list(method_args);
                        node!("call-assoc {o}::{name}{m}")
                    }
                    Callee::Ctor { owner, index } => {
                        let name = self.names.name_of_symbol(*owner);
                        node!("call-ctor {name}#{index}")
                    }
                    Callee::Value(_) => node!("call-value"),
                }
                self.nest(|d| {
                    match &callee {
                        Callee::Method { recv, .. } | Callee::BaseMethod { recv, .. } => {
                            d.expr(*recv)
                        }
                        Callee::Value(v) => d.expr(*v),
                        _ => {}
                    }
                    for a in args {
                        d.expr(a);
                    }
                });
            }
            ExprKind::Move(inner) => {
                node!("move");
                self.nest(|d| d.expr(inner));
            }
            ExprKind::Block(b) => {
                node!("block-value");
                self.nest(|d| d.block(b));
            }
            ExprKind::If { cond, then, else_branch } => {
                node!("if");
                self.nest(|d| {
                    d.expr(cond);
                    d.block(then);
                    if let Some(e) = else_branch {
                        d.block(e);
                    }
                });
            }
            ExprKind::IfLet { binding, operand, then, else_branch } => {
                node!("if-let");
                self.nest(|d| {
                    d.expr(operand);
                    let name = d.opt_local(&Some(binding));
                    d.line(format_args!("bind {name}"));
                    d.block(then);
                    if let Some(e) = else_branch {
                        d.block(e);
                    }
                });
            }
            ExprKind::Guarded { guard, init, value } => {
                node!("guarded");
                self.nest(|d| {
                    let name = d.opt_local(&Some(guard));
                    d.line(format_args!("guard {name}"));
                    d.expr(init);
                    d.expr(value);
                });
            }
            ExprKind::Formatted { args, params, body } => {
                node!("formatted");
                self.nest(|d| {
                    for (a, p) in args.iter().zip(&params) {
                        let name = d.opt_local(&Some(*p));
                        d.line(format_args!("bind {name}"));
                        d.nest(|d| d.expr(*a));
                    }
                    d.block(body);
                });
            }
            ExprKind::Switch { subject, arms, default } => {
                node!("switch");
                self.nest(|d| {
                    d.expr(subject);
                    for arm in &arms {
                        let vs: Vec<String> =
                            arm.variants.iter().map(|v| format!("#{v}")).collect();
                        let binding = d.opt_local(&arm.binding);
                        d.line(format_args!("case {} bind {binding}", vs.join(", ")));
                        d.nest(|d| {
                            for v in &arm.values {
                                d.expr(*v);
                            }
                            d.block(arm.body)
                        });
                    }
                    if let Some(b) = default {
                        d.line(format_args!("default"));
                        d.nest(|d| d.block(b));
                    }
                });
            }
            ExprKind::Or { operand, clause } => {
                node!("or");
                self.nest(|d| {
                    d.expr(operand);
                    match clause {
                        OrClause::Value(v) => d.expr(v),
                        OrClause::Block { binding, body } => {
                            let name = d.opt_local(&binding);
                            d.line(format_args!("clause {name}"));
                            d.nest(|d| d.block(body));
                        }
                        OrClause::Ignore => d.line(format_args!("ignore")),
                        OrClause::Unreachable => d.line(format_args!("unreachable")),
                    }
                });
            }
            ExprKind::Return(v) => {
                node!("return");
                self.nest(|d| {
                    if let Some(v) = v {
                        d.expr(v);
                    }
                });
            }
            ExprKind::Break(target) => match target {
                Some(id) => node!("break to #{id}"),
                None => node!("break"),
            },
            ExprKind::Continue(target) => match target {
                Some(id) => node!("continue to #{id}"),
                None => node!("continue"),
            },
            ExprKind::Len(base) => {
                node!("len");
                self.nest(|d| d.expr(base));
            }
            ExprKind::DataPtr(base) => {
                node!("data-ptr");
                self.nest(|d| d.expr(base));
            }
            ExprKind::TypeQuery { ty: queried, query } => {
                let t = self.names.name_of_type(&queried);
                node!("type-query {t}::{}", query.name())
            }
            ExprKind::Reflect { ty: reflected, info } => {
                let t = self.names.name_of_type(&reflected);
                node!("{} {t}", if info { "typeinfo-of" } else { "typeid-of" })
            }
            ExprKind::Builtin { name, args } => {
                node!("builtin {name}");
                self.nest(|d| {
                    for a in args {
                        d.expr(a);
                    }
                });
            }
            ExprKind::Intrinsic { name, args } => {
                node!("intrinsic #{name}");
                self.nest(|d| {
                    for a in args {
                        d.expr(a);
                    }
                });
            }
            ExprKind::DefaultArg(index) => node!("default-arg {index}"),
        }
    }
}

/// A loop's label as the dump shows it: nothing when no nested loop jumps to it.
fn label_str(label: &Option<LoopLabel>) -> String {
    match label {
        Some(l) => format!(" '{}#{}", l.name, l.id),
        None => String::new(),
    }
}
