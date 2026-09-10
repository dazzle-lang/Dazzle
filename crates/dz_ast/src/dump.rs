//! A span-free textual dump of the AST.
//!
//! Span-free on purpose: it is both the `dz parse` output and the structural comparison
//! used by the formatter's round-trip test, and re-formatting legitimately moves every
//! byte offset while it must not change the tree.

use std::fmt::Write;

use crate::*;

pub fn dump(src: &str, ast: &SourceFileAst) -> String {
    let mut d = Dumper { src, out: String::new(), indent: 0 };
    for import in &ast.imports {
        d.import(import);
    }
    for item in &ast.items {
        d.item(item);
    }
    d.out
}

struct Dumper<'a> {
    src: &'a str,
    out: String,
    indent: usize,
}

impl<'a> Dumper<'a> {
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

    fn text(&self, span: Span) -> &'a str {
        &self.src[span.range()]
    }

    // --- items ------------------------------------------------------------

    fn import(&mut self, i: &Import) {
        match &i.kind {
            ImportKind::Module { path, alias } => self.line(format_args!(
                "import {}{}",
                path_str(path),
                alias.as_ref().map(|a| format!(" as {}", a.name)).unwrap_or_default()
            )),
            ImportKind::Selective { path, names } => {
                let names: Vec<&str> = names.iter().map(|n| n.name.as_str()).collect();
                self.line(format_args!("import {}::{{{}}}", path_str(path), names.join(", ")));
            }
            ImportKind::Cpp { header, alias } => self.line(format_args!(
                "import cpp {}{}",
                self.text(*header),
                alias.as_ref().map(|a| format!(" as {}", a.name)).unwrap_or_default()
            )),
            ImportKind::Error => self.line(format_args!("import <error>")),
        }
    }

    fn item(&mut self, item: &Item) {
        for a in &item.attrs {
            self.line(format_args!("attr {}", a.name.name));
        }
        let vis = vis_str(item.vis);
        match &item.kind {
            ItemKind::Fn(f) => self.fn_decl("fn", vis, f),
            ItemKind::Struct(s) => {
                self.line(format_args!(
                    "{vis}struct {}{}{}",
                    s.name.name,
                    generics_str(&s.generics),
                    traits_str(&s.traits)
                ));
                self.nest(|d| {
                    for m in &s.members {
                        d.member(m);
                    }
                });
            }
            ItemKind::Union(u) => {
                self.line(format_args!(
                    "{vis}union {}{}",
                    u.name.name,
                    generics_str(&u.generics)
                ));
                self.nest(|d| {
                    for v in &u.variants {
                        d.line(format_args!("variant {}", v.name.name));
                        d.nest(|d| {
                            for f in &v.fields {
                                d.field(f);
                            }
                        });
                    }
                });
            }
            ItemKind::Enum(e) => {
                self.line(format_args!(
                    "{vis}enum {}{}",
                    e.name.name,
                    e.backing.as_ref().map(|t| format!(" : {}", type_str(t))).unwrap_or_default()
                ));
                self.nest(|d| {
                    for v in &e.variants {
                        match &v.value {
                            Some(expr) => {
                                d.line(format_args!("variant {} =", v.name.name));
                                d.nest(|d| d.expr(expr));
                            }
                            None => d.line(format_args!("variant {}", v.name.name)),
                        }
                    }
                });
            }
            ItemKind::Trait(t) => {
                self.line(format_args!(
                    "{vis}trait {}{}{}",
                    t.name.name,
                    generics_str(&t.generics),
                    traits_str(&t.supertraits)
                ));
                self.nest(|d| {
                    for m in &t.items {
                        d.member(m);
                    }
                });
            }
            ItemKind::Const(c) => {
                self.line(format_args!(
                    "{vis}const {}{}",
                    c.name.name,
                    c.ty.as_ref().map(|t| format!(" : {}", type_str(t))).unwrap_or_default()
                ));
                self.nest(|d| d.expr(&c.value));
            }
            ItemKind::Macro(m) => {
                self.line(format_args!("macro {} -> {}", m.name.name, m.kind.name))
            }
            ItemKind::MacroInvocation { name, .. } => {
                self.line(format_args!("macro-invocation #{}", name.name))
            }
            ItemKind::Error => self.line(format_args!("<error item>")),
        }
    }

    fn member(&mut self, m: &Member) {
        let vis = vis_str(m.vis);
        match &m.kind {
            MemberKind::Field(f) => self.field(f),
            MemberKind::Fn(f) => self.fn_decl("fn", vis, f),
            MemberKind::Ctor(f) => self.fn_decl("ctor", vis, f),
            MemberKind::Copy(f) => self.fn_decl("copy", vis, f),
            MemberKind::Drop(f) => self.fn_decl("drop", vis, f),
            MemberKind::TypeAlias { name, ty } => self.line(format_args!(
                "type {}{}",
                name.name,
                ty.as_ref().map(|t| format!(" = {}", type_str(t))).unwrap_or_default()
            )),
            MemberKind::Error => self.line(format_args!("<error member>")),
        }
    }

    fn field(&mut self, f: &Field) {
        self.line(format_args!(
            "field {}{}: {}",
            if f.is_own { "own " } else { "" },
            f.name.name,
            type_str(&f.ty)
        ));
        if let Some(d) = &f.default {
            self.nest(|dd| {
                dd.line(format_args!("default"));
                dd.nest(|dd| dd.expr(d));
            });
        }
    }

    fn fn_decl(&mut self, what: &str, vis: &str, f: &FnDecl) {
        let params: Vec<String> = f
            .params
            .iter()
            .map(|p| {
                format!("{}: {}{}", p.name.name, type_str(&p.ty), if p.default.is_some() { " = ..." } else { "" })
            })
            .collect();
        let recv = match &f.self_param {
            Some(s) if s.is_mut => "&mut self",
            Some(_) => "&self",
            None => "",
        };
        let all = if recv.is_empty() {
            params.join(", ")
        } else if params.is_empty() {
            recv.to_owned()
        } else {
            format!("{recv}, {}", params.join(", "))
        };
        self.line(format_args!(
            "{vis}{}{} {}{}({}){}",
            if f.is_mut { "mut " } else { "" },
            what,
            f.name.name,
            generics_str(&f.generics),
            all,
            f.ret
                .as_ref()
                .map(|t| format!(" {} {}", arrow(f.fallible), type_str(t)))
                .unwrap_or_else(|| if f.fallible { " !->".to_owned() } else { String::new() })
        ));
        for p in &f.params {
            if let Some(default) = &p.default {
                self.nest(|d| {
                    d.line(format_args!("param-default {}", p.name.name));
                    d.nest(|d| d.expr(default));
                });
            }
        }
        if let Some(body) = &f.body {
            self.nest(|d| d.block(body));
        }
    }

    // --- statements -------------------------------------------------------

    fn block(&mut self, b: &Block) {
        self.line(format_args!("block"));
        self.nest(|d| {
            for s in &b.stmts {
                d.stmt(s);
            }
        });
    }

    fn stmt(&mut self, s: &Stmt) {
        match &s.kind {
            StmtKind::Let(l) => {
                self.line(format_args!(
                    "let{} {}{}",
                    if l.mutable { " mut" } else { "" },
                    pat_str(&l.pat),
                    l.ty.as_ref().map(|t| format!(": {}", type_str(t))).unwrap_or_default()
                ));
                if let Some(init) = &l.init {
                    self.nest(|d| d.expr(init));
                }
            }
            StmtKind::Assign { target, op, value } => {
                self.line(format_args!("assign {}", op.as_str()));
                self.nest(|d| {
                    d.expr(target);
                    d.expr(value);
                });
            }
            StmtKind::Expr(e) => self.expr(e),
            StmtKind::While(w) => {
                self.line(format_args!(
                    "while{}",
                    w.label.as_ref().map(|l| format!(" '{}", l.name)).unwrap_or_default()
                ));
                self.nest(|d| {
                    d.expr(&w.cond);
                    d.block(&w.body);
                });
            }
            StmtKind::For(f) => {
                let binds: Vec<String> = f.bindings.iter().map(pat_str).collect();
                self.line(format_args!(
                    "for{} {}",
                    f.label.as_ref().map(|l| format!(" '{}", l.name)).unwrap_or_default(),
                    binds.join(", ")
                ));
                self.nest(|d| {
                    d.expr(&f.iter);
                    d.block(&f.body);
                });
            }
            StmtKind::Defer(inner) => {
                self.line(format_args!("defer"));
                self.nest(|d| d.stmt(inner));
            }
            StmtKind::Item(item) => self.item(item),
            StmtKind::Error => self.line(format_args!("<error stmt>")),
        }
    }

    // --- expressions ------------------------------------------------------

    fn expr(&mut self, e: &Expr) {
        match &e.kind {
            ExprKind::Int(v) => self.line(format_args!("int {v}")),
            ExprKind::Float(v) => self.line(format_args!("float {v}")),
            ExprKind::Str(v) => self.line(format_args!("str {v}")),
            ExprKind::RawStr(v) => self.line(format_args!("rawstr {v}")),
            ExprKind::Char(v) => self.line(format_args!("char {v}")),
            ExprKind::Bool(v) => self.line(format_args!("bool {v}")),
            ExprKind::Null => self.line(format_args!("null")),
            ExprKind::None => self.line(format_args!("none")),
            ExprKind::Path(p) => self.line(format_args!("path {}", path_str(p))),
            ExprKind::DotInfer { name, init } => {
                self.line(format_args!(".{}", name.name));
                if let Some(fields) = init {
                    self.nest(|d| d.field_inits(fields));
                }
            }
            ExprKind::StructLit { path, fields } => {
                self.line(format_args!("struct-lit {}", path_str(path)));
                self.nest(|d| d.field_inits(fields));
            }
            ExprKind::Call { callee, args } => {
                self.line(format_args!("call"));
                self.nest(|d| {
                    d.expr(callee);
                    for a in args {
                        d.expr(a);
                    }
                });
            }
            ExprKind::Index { base, index } => {
                self.line(format_args!("index"));
                self.nest(|d| {
                    d.expr(base);
                    d.expr(index);
                });
            }
            ExprKind::Field { base, name } => {
                self.line(format_args!("field .{}", name.name));
                self.nest(|d| d.expr(base));
            }
            ExprKind::Unary { op, operand } => {
                self.line(format_args!("unary {}", op.as_str()));
                self.nest(|d| d.expr(operand));
            }
            ExprKind::Binary { op, lhs, rhs } => {
                self.line(format_args!("binary {}", op.as_str()));
                self.nest(|d| {
                    d.expr(lhs);
                    d.expr(rhs);
                });
            }
            ExprKind::Cast { expr, ty } => {
                self.line(format_args!("cast {}", type_str(ty)));
                self.nest(|d| d.expr(expr));
            }
            ExprKind::Range { start, end, inclusive } => {
                self.line(format_args!("range{}", if *inclusive { "=" } else { "" }));
                self.nest(|d| {
                    match start {
                        Some(s) => d.expr(s),
                        None => d.line(format_args!("<open>")),
                    }
                    match end {
                        Some(s) => d.expr(s),
                        None => d.line(format_args!("<open>")),
                    }
                });
            }
            ExprKind::Ref { mutable, expr } => {
                self.line(format_args!("ref{}", if *mutable { " mut" } else { "" }));
                self.nest(|d| d.expr(expr));
            }
            ExprKind::Deref(e) => {
                self.line(format_args!("deref"));
                self.nest(|d| d.expr(e));
            }
            ExprKind::Move(e) => {
                self.line(format_args!("move"));
                self.nest(|d| d.expr(e));
            }
            ExprKind::Or { expr, clause } => {
                self.line(format_args!("or"));
                self.nest(|d| {
                    d.expr(expr);
                    match clause.as_ref() {
                        OrClause::Value(v) => d.expr(v),
                        OrClause::Block { binding, body } => {
                            match binding {
                                Some(b) => d.line(format_args!("clause {}", b.name)),
                                None => d.line(format_args!("clause")),
                            }
                            d.nest(|d| d.block(body));
                        }
                        OrClause::Ignore(_) => d.line(format_args!("ignore")),
                        OrClause::Unreachable(_) => d.line(format_args!("unreachable")),
                    }
                });
            }
            ExprKind::Block(b) => self.block(b),
            ExprKind::If(i) => {
                self.line(format_args!("if"));
                self.nest(|d| {
                    d.expr(&i.cond);
                    d.block(&i.then);
                    if let Some(e) = &i.else_branch {
                        d.line(format_args!("else"));
                        d.nest(|d| d.expr(e));
                    }
                });
            }
            ExprKind::Switch(s) => {
                self.line(format_args!("switch"));
                self.nest(|d| {
                    d.expr(&s.subject);
                    for arm in &s.arms {
                        let pats: Vec<String> = arm
                            .patterns
                            .iter()
                            .map(|p| match &p.binding {
                                Some(b) => format!("{}({})", p.name.name, b.name),
                                None => p.name.name.clone(),
                            })
                            .collect();
                        d.line(format_args!("case {}", pats.join(", ")));
                        d.nest(|d| d.block(&arm.body));
                    }
                    if let Some(def) = &s.default {
                        d.line(format_args!("default"));
                        d.nest(|d| d.block(def));
                    }
                });
            }
            ExprKind::Return(v) => {
                self.line(format_args!("return"));
                if let Some(v) = v {
                    self.nest(|d| d.expr(v));
                }
            }
            ExprKind::Break { label, value } => {
                self.line(format_args!(
                    "break{}",
                    label.as_ref().map(|l| format!(" '{}", l.name)).unwrap_or_default()
                ));
                if let Some(v) = value {
                    self.nest(|d| d.expr(v));
                }
            }
            ExprKind::Continue { label } => self.line(format_args!(
                "continue{}",
                label.as_ref().map(|l| format!(" '{}", l.name)).unwrap_or_default()
            )),
            ExprKind::Intrinsic { name, args } => {
                self.line(format_args!("intrinsic #{}", name.name));
                self.nest(|d| {
                    for a in args {
                        d.expr(a);
                    }
                });
            }
            ExprKind::MacroCall { name, .. } => self.line(format_args!("macro-call {}!", name.name)),
            ExprKind::Tuple(elems) => {
                self.line(format_args!("tuple"));
                self.nest(|d| {
                    for e in elems {
                        d.expr(e);
                    }
                });
            }
            ExprKind::Error => self.line(format_args!("<error expr>")),
        }
    }

    fn field_inits(&mut self, fields: &[FieldInit]) {
        for f in fields {
            match &f.name {
                Some(n) => {
                    self.line(format_args!("field-init {}", n.name));
                    self.nest(|d| d.expr(&f.value));
                }
                None => self.expr(&f.value),
            }
        }
    }
}

// --- helpers ---------------------------------------------------------------

fn vis_str(v: Vis) -> &'static str {
    match v {
        Vis::Private => "",
        Vis::Module => "pub(module) ",
        Vis::Public => "pub ",
    }
}

pub fn path_str(p: &Path) -> String {
    p.segments
        .iter()
        .map(|s| {
            if s.generics.is_empty() {
                s.name.name.clone()
            } else {
                let args: Vec<String> = s.generics.iter().map(type_str).collect();
                format!("{}<{}>", s.name.name, args.join(", "))
            }
        })
        .collect::<Vec<_>>()
        .join("::")
}

fn generics_str(g: &Generics) -> String {
    if g.is_empty() {
        return String::new();
    }
    let params: Vec<String> = g
        .params
        .iter()
        .map(|p| {
            if p.bounds.is_empty() {
                p.name.name.clone()
            } else {
                let bounds: Vec<String> = p.bounds.iter().map(path_str).collect();
                format!("{}: {}", p.name.name, bounds.join(" + "))
            }
        })
        .collect();
    format!("<{}>", params.join(", "))
}

fn traits_str(traits: &[Path]) -> String {
    if traits.is_empty() {
        String::new()
    } else {
        format!(" : {}", traits.iter().map(path_str).collect::<Vec<_>>().join(" + "))
    }
}

/// `->`, or `!->` when the signature is fallible (spec §9).
fn arrow(fallible: bool) -> &'static str {
    if fallible {
        "!->"
    } else {
        "->"
    }
}

pub fn type_str(t: &Type) -> String {
    match &t.kind {
        TypeKind::Path(p) => path_str(p),
        TypeKind::Ptr(inner) => format!("^{}", type_str(inner)),
        TypeKind::OwnPtr(inner) => format!("own ^{}", type_str(inner)),
        TypeKind::Ref { mutable, inner } => {
            format!("&{}{}", if *mutable { "mut " } else { "" }, type_str(inner))
        }
        TypeKind::Optional(inner) => format!("{}?", type_str(inner)),
        TypeKind::Array { size, elem } => match size {
            Some(_) => format!("[N]{}", type_str(elem)),
            None => format!("[]{}", type_str(elem)),
        },
        TypeKind::Tuple(elems) => {
            format!("({})", elems.iter().map(type_str).collect::<Vec<_>>().join(", "))
        }
        TypeKind::Fn { params, ret } => format!(
            "fn({}){}",
            params.iter().map(type_str).collect::<Vec<_>>().join(", "),
            ret.as_ref().map(|r| format!(" -> {}", type_str(r))).unwrap_or_default()
        ),
        TypeKind::SelfType => "Self".to_owned(),
        TypeKind::Error => "<error type>".to_owned(),
    }
}

pub fn pat_str(p: &Pat) -> String {
    match &p.kind {
        PatKind::Ident(i) => i.name.clone(),
        PatKind::Wild => "_".to_owned(),
        PatKind::Tuple(elems) => {
            format!("({})", elems.iter().map(pat_str).collect::<Vec<_>>().join(", "))
        }
        PatKind::Error => "<error pat>".to_owned(),
    }
}
