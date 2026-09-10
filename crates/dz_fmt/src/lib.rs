//! The Dazzle formatter (`dz fmt`, spec §17.3).
//!
//! Printing is driven entirely by the AST, with two things layered back on top that an
//! AST does not carry:
//!
//! * **Comments**, taken from the lexer's side table and re-attached by source position.
//!   A formatter that eats comments is worse than no formatter.
//! * **Blank lines**, preserved (collapsed to at most one) between declarations and
//!   statements, because they are the author's paragraph breaks.
//!
//! Every layout decision is a pure function of the AST and the current indent, which is
//! what makes formatting idempotent.

use dz_ast::*;
use dz_lex::{Comment, CommentKind};
use dz_span::{SourceFile, Span};

/// Soft limit before argument lists and parameter lists break one-per-line.
const MAX_WIDTH: usize = 100;
const INDENT: &str = "    ";

pub fn format(file: &SourceFile, ast: &SourceFileAst, comments: &[Comment]) -> String {
    let mut p = Printer {
        file,
        src: file.text(),
        comments,
        next_comment: 0,
        out: String::new(),
        indent: 0,
        pending_blank: false,
    };
    p.file_ast(ast);
    p.finish()
}

struct Printer<'a> {
    file: &'a SourceFile,
    src: &'a str,
    comments: &'a [Comment],
    next_comment: usize,
    out: String,
    indent: usize,
    pending_blank: bool,
}

impl<'a> Printer<'a> {
    // --- output primitives ------------------------------------------------

    fn write_indent(&mut self) {
        for _ in 0..self.indent {
            self.out.push_str(INDENT);
        }
    }

    /// Emit a complete line at the current indent.
    fn line(&mut self, text: &str) {
        self.flush_blank();
        self.write_indent();
        self.out.push_str(text);
        self.out.push('\n');
    }

    fn flush_blank(&mut self) {
        if self.pending_blank {
            self.pending_blank = false;
            if !self.out.is_empty() && !self.out.ends_with("\n\n") {
                self.out.push('\n');
            }
        }
    }

    fn line_of(&self, offset: u32) -> u32 {
        self.file.location(offset as usize).line
    }

    fn text(&self, span: Span) -> &'a str {
        &self.src[span.range()]
    }

    fn finish(mut self) -> String {
        // Anything after the last declaration.
        self.emit_comments_before(u32::MAX, false);
        while self.out.ends_with("\n\n") {
            self.out.pop();
        }
        if !self.out.ends_with('\n') && !self.out.is_empty() {
            self.out.push('\n');
        }
        self.out
    }

    // --- comments ---------------------------------------------------------

    /// Emit every pending comment that starts before `offset`.
    ///
    /// `keep_blanks` preserves a single blank line where the source had one, so comments
    /// stay attached to whatever they were written above.
    fn emit_comments_before(&mut self, offset: u32, keep_blanks: bool) {
        while self.next_comment < self.comments.len() {
            let c = self.comments[self.next_comment];
            if c.span.start >= offset {
                break;
            }
            self.next_comment += 1;

            if keep_blanks && self.blank_line_before(c.span.start) {
                self.pending_blank = true;
            }
            match c.kind {
                CommentKind::Line => {
                    let text = self.text(c.span).trim_end().to_owned();
                    self.line(&text);
                }
                CommentKind::Block => {
                    // Block comments are reproduced verbatim; re-indenting their interior
                    // would corrupt ASCII art and commented-out code.
                    let text = self.text(c.span).to_owned();
                    self.flush_blank();
                    self.write_indent();
                    self.out.push_str(&text);
                    self.out.push('\n');
                }
            }
        }
    }

    /// A trailing comment on the same source line as `end`, e.g. `let a = 1 // why`.
    fn emit_trailing_comment(&mut self, end: u32) {
        if self.next_comment >= self.comments.len() {
            return;
        }
        let c = self.comments[self.next_comment];
        if c.kind != CommentKind::Line || self.line_of(c.span.start) != self.line_of(end) {
            return;
        }
        self.next_comment += 1;
        let text = self.text(c.span).trim_end().to_owned();
        // Replace the newline the caller just wrote.
        if self.out.ends_with('\n') {
            self.out.pop();
        }
        self.out.push(' ');
        self.out.push_str(&text);
        self.out.push('\n');
    }

    /// Did the author leave a blank line immediately before `offset`?
    fn blank_line_before(&self, offset: u32) -> bool {
        let upto = &self.src[..offset as usize];
        let trimmed = upto.trim_end_matches([' ', '\t']);
        let mut newlines = 0;
        for c in trimmed.chars().rev() {
            match c {
                '\n' => newlines += 1,
                ' ' | '\t' | '\r' => {}
                _ => break,
            }
            if newlines >= 2 {
                return true;
            }
        }
        false
    }

    /// Common prologue for anything with docs, attributes and a leading blank line.
    fn prologue(&mut self, span: Span, docs: &Docs, attrs: &[Attr], first: bool) {
        let anchor = docs.first().map(|d| d.start).unwrap_or(span.start);
        self.emit_comments_before(anchor, !first);
        if !first && self.blank_line_before(anchor) {
            self.pending_blank = true;
        }
        for d in docs {
            let text = self.text(*d).trim_end().to_owned();
            self.line(&text);
        }
        for a in attrs {
            let rendered = self.attr(a);
            self.line(&rendered);
        }
    }

    fn attr(&self, a: &Attr) -> String {
        if a.args.is_empty() {
            format!("#[{}]", a.name.name)
        } else {
            let args: Vec<String> = a.args.iter().map(|x| self.attr_arg(x)).collect();
            format!("#[{}({})]", a.name.name, args.join(", "))
        }
    }

    fn attr_arg(&self, a: &AttrArg) -> String {
        match a {
            AttrArg::Ident(i) => i.name.clone(),
            AttrArg::Literal(e) => self.expr(e),
            AttrArg::KeyValue { key, value } => format!("{} = {}", key.name, self.expr(value)),
            AttrArg::Nested(inner) => {
                let s = self.attr(inner);
                s.trim_start_matches("#[").trim_end_matches(']').to_owned()
            }
        }
    }

    // --- file and items ---------------------------------------------------

    fn file_ast(&mut self, ast: &SourceFileAst) {
        let mut first = true;
        for import in &ast.imports {
            self.emit_comments_before(import.span.start, !first);
            if !first && self.blank_line_before(import.span.start) {
                self.pending_blank = true;
            }
            let text = self.import(import);
            self.line(&text);
            self.emit_trailing_comment(import.span.end);
            first = false;
        }
        for item in &ast.items {
            if !first {
                self.pending_blank = true;
            }
            self.item(item, first);
            first = false;
        }
    }

    fn import(&self, i: &Import) -> String {
        match &i.kind {
            ImportKind::Module { path, alias } => format!(
                "import {}{}",
                dump::path_str(path),
                alias.as_ref().map(|a| format!(" as {}", a.name)).unwrap_or_default()
            ),
            ImportKind::Selective { path, names } => format!(
                "import {}::{{ {} }}",
                dump::path_str(path),
                names.iter().map(|n| n.name.as_str()).collect::<Vec<_>>().join(", ")
            ),
            ImportKind::Cpp { header, alias } => format!(
                "import cpp {}{}",
                self.text(*header),
                alias.as_ref().map(|a| format!(" as {}", a.name)).unwrap_or_default()
            ),
            ImportKind::Error => "import <error>".to_owned(),
        }
    }

    fn item(&mut self, item: &Item, first: bool) {
        self.prologue(item.span, &item.docs, &item.attrs, first);
        let vis = vis_str(item.vis);
        match &item.kind {
            ItemKind::Fn(f) => self.fn_decl(vis, f),
            ItemKind::Struct(s) => {
                let head = format!(
                    "{vis}struct {}{}{} {{",
                    s.name.name,
                    generics_str(&s.generics),
                    traits_str(&s.traits)
                );
                self.line(&head);
                self.indent += 1;
                self.members(&s.members);
                self.indent -= 1;
                self.line("}");
            }
            ItemKind::Union(u) => {
                self.line(&format!("{vis}union {}{} {{", u.name.name, generics_str(&u.generics)));
                self.indent += 1;
                for (i, v) in u.variants.iter().enumerate() {
                    self.emit_comments_before(v.span.start, i > 0);
                    if i > 0 && self.blank_line_before(v.span.start) {
                        self.pending_blank = true;
                    }
                    for d in &v.docs {
                        let t = self.text(*d).trim_end().to_owned();
                        self.line(&t);
                    }
                    if v.has_payload {
                        let fields: Vec<String> = v
                            .fields
                            .iter()
                            .map(|f| format!("{}: {}", f.name.name, self.ty(&f.ty)))
                            .collect();
                        self.line(&format!("{} {{ {} }}", v.name.name, fields.join(", ")));
                    } else {
                        self.line(&v.name.name);
                    }
                    self.emit_trailing_comment(v.span.end);
                }
                self.indent -= 1;
                self.line("}");
            }
            ItemKind::Enum(e) => {
                let backing =
                    e.backing.as_ref().map(|t| format!(" : {}", self.ty(t))).unwrap_or_default();
                self.line(&format!("{vis}enum {}{} {{", e.name.name, backing));
                self.indent += 1;
                for (i, v) in e.variants.iter().enumerate() {
                    self.emit_comments_before(v.span.start, i > 0);
                    if i > 0 && self.blank_line_before(v.span.start) {
                        self.pending_blank = true;
                    }
                    for d in &v.docs {
                        let t = self.text(*d).trim_end().to_owned();
                        self.line(&t);
                    }
                    match &v.value {
                        Some(val) => {
                            let s = format!("{} = {}", v.name.name, self.expr(val));
                            self.line(&s);
                        }
                        None => self.line(&v.name.name),
                    }
                    self.emit_trailing_comment(v.span.end);
                }
                self.indent -= 1;
                self.line("}");
            }
            ItemKind::Trait(t) => {
                self.line(&format!(
                    "{vis}trait {}{}{} {{",
                    t.name.name,
                    generics_str(&t.generics),
                    traits_str(&t.supertraits)
                ));
                self.indent += 1;
                self.members(&t.items);
                self.indent -= 1;
                self.line("}");
            }
            ItemKind::Const(c) => {
                let ty = c.ty.as_ref().map(|t| format!(": {}", self.ty(t))).unwrap_or_default();
                let s = format!("{vis}const {}{} = {}", c.name.name, ty, self.expr(&c.value));
                self.line(&s);
                self.emit_trailing_comment(item.span.end);
            }
            // Macro bodies are not parsed until M10, so they are reproduced verbatim.
            ItemKind::Macro(m) => {
                let body = self.text(m.body).to_owned();
                let s = format!("macro {} -> {} {}", m.name.name, m.kind.name, body);
                self.verbatim_block(&s);
            }
            ItemKind::MacroInvocation { name, body } => {
                let raw = self.text(Span { start: item.span.start, end: body.end }).to_owned();
                self.verbatim_block(&raw);
                let _ = name;
            }
            ItemKind::Error => self.line("/* <error> */"),
        }
    }

    /// Emit multi-line raw source, re-indenting only its first line.
    fn verbatim_block(&mut self, text: &str) {
        self.flush_blank();
        let mut lines = text.lines();
        if let Some(first) = lines.next() {
            self.write_indent();
            self.out.push_str(first);
            self.out.push('\n');
        }
        for l in lines {
            self.out.push_str(l);
            self.out.push('\n');
        }
    }

    fn members(&mut self, members: &[Member]) {
        // Consecutive fields get their types aligned, which is the style the language's
        // own samples use. The run ends at any non-field member or a blank line.
        let mut i = 0;
        let mut first = true;
        while i < members.len() {
            let run_end = field_run_end(members, i, self);
            if run_end > i + 1 {
                let width = members[i..run_end]
                    .iter()
                    .filter_map(|m| match &m.kind {
                        MemberKind::Field(f) => {
                            Some(f.name.name.chars().count() + if f.is_own { 4 } else { 0 })
                        }
                        _ => None,
                    })
                    .max()
                    .unwrap_or(0);
                for m in &members[i..run_end] {
                    self.member(m, first, width);
                    first = false;
                }
                i = run_end;
            } else {
                self.member(&members[i], first, 0);
                first = false;
                i += 1;
            }
        }
    }

    fn member(&mut self, m: &Member, first: bool, align: usize) {
        self.prologue(m.span, &m.docs, &m.attrs, first);
        let vis = vis_str(m.vis);
        match &m.kind {
            MemberKind::Field(f) => {
                let own = if f.is_own { "own " } else { "" };
                let lhs = format!("{own}{}", f.name.name);
                let pad = align.saturating_sub(lhs.chars().count());
                let default =
                    f.default.as_ref().map(|d| format!(" = {}", self.expr(d))).unwrap_or_default();
                let s = format!("{vis}{lhs}:{} {}{default}", " ".repeat(pad), self.ty(&f.ty));
                self.line(&s);
                self.emit_trailing_comment(m.span.end);
            }
            MemberKind::Fn(f) => self.fn_decl(vis, f),
            MemberKind::Ctor(f) | MemberKind::Copy(f) | MemberKind::Drop(f) => {
                self.special_member(vis, f)
            }
            MemberKind::TypeAlias { name, ty } => {
                let s = format!(
                    "{vis}type {}{}",
                    name.name,
                    ty.as_ref().map(|t| format!(" = {}", self.ty(t))).unwrap_or_default()
                );
                self.line(&s);
                self.emit_trailing_comment(m.span.end);
            }
            MemberKind::Error => self.line("/* <error member> */"),
        }
    }

    fn fn_decl(&mut self, vis: &str, f: &FnDecl) {
        let head = format!(
            "{vis}{}fn {}{}",
            if f.is_mut { "mut " } else { "" },
            f.name.name,
            generics_str(&f.generics)
        );
        self.signature(&head, f);
    }

    fn special_member(&mut self, vis: &str, f: &FnDecl) {
        let head = format!("{vis}{}", f.name.name);
        self.signature(&head, f);
    }

    fn signature(&mut self, head: &str, f: &FnDecl) {
        let mut params: Vec<String> = Vec::new();
        if let Some(s) = &f.self_param {
            params.push(if s.is_mut { "&mut self".to_owned() } else { "&self".to_owned() });
        }
        for p in &f.params {
            let default =
                p.default.as_ref().map(|d| format!(" = {}", self.expr(d))).unwrap_or_default();
            params.push(format!("{}: {}{default}", p.name.name, self.ty(&p.ty)));
        }
        let ret = f.ret.as_ref().map(|t| format!(" -> {}", self.ty(t))).unwrap_or_default();
        let empty_body = f.body.as_ref().is_some_and(|b| b.stmts.is_empty());
        let open = match &f.body {
            Some(_) if empty_body => " {}",
            Some(_) => " {",
            None => "",
        };

        let inline = format!("{head}({}){ret}{open}", params.join(", "));
        if self.indent * INDENT.len() + inline.chars().count() <= MAX_WIDTH || params.is_empty() {
            self.line(&inline);
        } else {
            self.line(&format!("{head}("));
            self.indent += 1;
            for (i, p) in params.iter().enumerate() {
                let comma = if i + 1 < params.len() { "," } else { "" };
                self.line(&format!("{p}{comma}"));
            }
            self.indent -= 1;
            self.line(&format!("){ret}{open}"));
        }

        if let Some(body) = &f.body {
            if !empty_body {
                self.indent += 1;
                self.block_body(body);
                self.indent -= 1;
                self.line("}");
            }
        }
    }

    // --- statements -------------------------------------------------------

    fn block_body(&mut self, b: &Block) {
        for (i, s) in b.stmts.iter().enumerate() {
            self.stmt(s, i == 0);
        }
        // Comments sitting at the end of the block, after the last statement.
        self.emit_comments_before(b.span.end, true);
    }

    fn stmt(&mut self, s: &Stmt, first: bool) {
        let anchor = s.docs.first().map(|d| d.start).unwrap_or(s.span.start);
        self.emit_comments_before(anchor, !first);
        if !first && self.blank_line_before(anchor) {
            self.pending_blank = true;
        }
        for d in &s.docs {
            let t = self.text(*d).trim_end().to_owned();
            self.line(&t);
        }

        match &s.kind {
            StmtKind::Let(l) => {
                let ty = l.ty.as_ref().map(|t| format!(": {}", self.ty(t))).unwrap_or_default();
                let init =
                    l.init.as_ref().map(|e| format!(" = {}", self.expr(e))).unwrap_or_default();
                let text = format!(
                    "let {}{}{ty}{init}",
                    if l.mutable { "mut " } else { "" },
                    dump::pat_str(&l.pat)
                );
                self.line(&text);
                self.emit_trailing_comment(s.span.end);
            }
            StmtKind::Assign { target, op, value } => {
                let text =
                    format!("{} {} {}", self.expr(target), op.as_str(), self.expr(value));
                self.line(&text);
                self.emit_trailing_comment(s.span.end);
            }
            StmtKind::Expr(e) => {
                self.expr_stmt(e);
                self.emit_trailing_comment(s.span.end);
            }
            StmtKind::While(w) => {
                let label =
                    w.label.as_ref().map(|l| format!("{}: ", l.name)).unwrap_or_default();
                let text = format!("{label}while {} {{", self.expr(&w.cond));
                self.line(&text);
                self.indent += 1;
                self.block_body(&w.body);
                self.indent -= 1;
                self.line("}");
            }
            StmtKind::For(f) => {
                let label =
                    f.label.as_ref().map(|l| format!("{}: ", l.name)).unwrap_or_default();
                let binds: Vec<String> = f.bindings.iter().map(dump::pat_str).collect();
                let text =
                    format!("{label}for {} in {} {{", binds.join(", "), self.expr(&f.iter));
                self.line(&text);
                self.indent += 1;
                self.block_body(&f.body);
                self.indent -= 1;
                self.line("}");
            }
            StmtKind::Defer(inner) => {
                // `defer` takes a statement, so render the inner one and prefix it. The
                // pending blank line has to be settled first, or it lands between `defer`
                // and its statement.
                self.flush_blank();
                let mark = self.out.len();
                let indent_at = self.indent;
                self.stmt(inner, true);
                let rendered = self.out.split_off(mark);
                let prefix = INDENT.repeat(indent_at);
                let body = rendered.strip_prefix(&prefix).unwrap_or(&rendered).to_owned();
                self.write_indent();
                self.out.push_str("defer ");
                self.out.push_str(&body);
            }
            StmtKind::Item(item) => self.item(item, first),
            StmtKind::Error => self.line("/* <error> */"),
        }
    }

    /// Statement-position expressions whose bodies are blocks print multi-line.
    fn expr_stmt(&mut self, e: &Expr) {
        match &e.kind {
            ExprKind::If(_) | ExprKind::Switch(_) | ExprKind::Block(_) => {
                let text = self.expr(e);
                self.emit_multiline(&text);
            }
            _ => {
                let text = self.expr(e);
                self.emit_multiline(&text);
            }
        }
    }

    /// Write pre-rendered text that may already contain newlines, indenting each line.
    fn emit_multiline(&mut self, text: &str) {
        self.flush_blank();
        for l in text.split('\n') {
            self.write_indent();
            self.out.push_str(l);
            self.out.push('\n');
        }
    }

    // --- types and expressions --------------------------------------------

    fn ty(&self, t: &Type) -> String {
        match &t.kind {
            TypeKind::Array { size: Some(n), elem } => {
                format!("[{}]{}", self.expr(n), self.ty(elem))
            }
            TypeKind::Array { size: None, elem } => format!("[]{}", self.ty(elem)),
            TypeKind::Ptr(i) => format!("^{}", self.ty(i)),
            TypeKind::OwnPtr(i) => format!("own ^{}", self.ty(i)),
            TypeKind::Ref { mutable, inner } => {
                format!("&{}{}", if *mutable { "mut " } else { "" }, self.ty(inner))
            }
            TypeKind::Optional(i) => format!("{}?", self.ty(i)),
            TypeKind::Tuple(elems) => {
                format!("({})", elems.iter().map(|e| self.ty(e)).collect::<Vec<_>>().join(", "))
            }
            TypeKind::Fn { params, ret } => format!(
                "fn({}){}",
                params.iter().map(|p| self.ty(p)).collect::<Vec<_>>().join(", "),
                ret.as_ref().map(|r| format!(" -> {}", self.ty(r))).unwrap_or_default()
            ),
            TypeKind::Path(p) => self.path(p),
            TypeKind::SelfType => "Self".to_owned(),
            TypeKind::Error => "/* <error type> */".to_owned(),
        }
    }

    fn path(&self, p: &Path) -> String {
        p.segments
            .iter()
            .map(|s| {
                if s.generics.is_empty() {
                    s.name.name.clone()
                } else {
                    let args: Vec<String> = s.generics.iter().map(|g| self.ty(g)).collect();
                    format!("{}<{}>", s.name.name, args.join(", "))
                }
            })
            .collect::<Vec<_>>()
            .join("::")
    }

    fn expr(&self, e: &Expr) -> String {
        self.expr_prec(e, 0)
    }

    /// Render `e`, parenthesising it when its own precedence is looser than `min`.
    fn expr_prec(&self, e: &Expr, min: u8) -> String {
        let prec = expr_precedence(e);
        let body = self.expr_inner(e);
        if prec < min {
            format!("({body})")
        } else {
            body
        }
    }

    fn expr_inner(&self, e: &Expr) -> String {
        match &e.kind {
            ExprKind::Int(v) | ExprKind::Float(v) | ExprKind::Str(v) | ExprKind::RawStr(v)
            | ExprKind::Char(v) => v.clone(),
            ExprKind::Bool(b) => b.to_string(),
            ExprKind::Null => "null".to_owned(),
            ExprKind::None => "none".to_owned(),
            ExprKind::Path(p) => self.path(p),
            ExprKind::DotInfer { name, init } => match init {
                Some(fields) => format!(".{}{}", name.name, self.struct_body(fields)),
                None => format!(".{}", name.name),
            },
            ExprKind::StructLit { path, fields } => {
                format!("{}{}", self.path(path), self.struct_body(fields))
            }
            ExprKind::Call { callee, args } => {
                let rendered: Vec<String> = args.iter().map(|a| self.expr(a)).collect();
                format!("{}({})", self.expr_prec(callee, 14), rendered.join(", "))
            }
            ExprKind::Index { base, index } => {
                format!("{}[{}]", self.expr_prec(base, 14), self.expr(index))
            }
            ExprKind::Field { base, name } => {
                format!("{}.{}", self.expr_prec(base, 14), name.name)
            }
            ExprKind::Unary { op, operand } => {
                format!("{}{}", op.as_str(), self.expr_prec(operand, 13))
            }
            ExprKind::Binary { op, lhs, rhs } => {
                let (l, r) = binary_prec(*op);
                format!(
                    "{} {} {}",
                    self.expr_prec(lhs, l),
                    op.as_str(),
                    self.expr_prec(rhs, r)
                )
            }
            ExprKind::Cast { expr, ty } => {
                format!("{} as {}", self.expr_prec(expr, 12), self.ty(ty))
            }
            ExprKind::Range { start, end, inclusive } => format!(
                "{}{}{}",
                start.as_ref().map(|s| self.expr_prec(s, 6)).unwrap_or_default(),
                if *inclusive { "..=" } else { ".." },
                end.as_ref().map(|s| self.expr_prec(s, 6)).unwrap_or_default()
            ),
            ExprKind::Ref { mutable, expr } => {
                format!("&{}{}", if *mutable { "mut " } else { "" }, self.expr_prec(expr, 13))
            }
            ExprKind::Deref(inner) => format!("{}^", self.expr_prec(inner, 14)),
            ExprKind::Move(inner) => format!("move({})", self.expr(inner)),
            ExprKind::Or { expr, clause } => {
                let c = match clause.as_ref() {
                    OrClause::Value(v) => self.expr_prec(v, 2),
                    OrClause::Block { binding, body } => self.closure_block(binding, body),
                    OrClause::Ignore(_) => "ignore".to_owned(),
                    OrClause::Unreachable(_) => "unreachable".to_owned(),
                };
                format!("{} or {c}", self.expr_prec(expr, 1))
            }
            ExprKind::Block(b) => self.render_block(b),
            ExprKind::If(i) => {
                let mut s = format!("if {} {}", self.expr(&i.cond), self.render_block(&i.then));
                if let Some(e) = &i.else_branch {
                    s.push_str(" else ");
                    s.push_str(&self.expr_inner(e));
                }
                s
            }
            ExprKind::Switch(sw) => {
                let mut s = format!("switch {} {{\n", self.expr(&sw.subject));
                for arm in &sw.arms {
                    let pats: Vec<String> = arm
                        .patterns
                        .iter()
                        .map(|p| match &p.binding {
                            Some(b) => format!("{}({})", p.name.name, b.name),
                            None => p.name.name.clone(),
                        })
                        .collect();
                    s.push_str(&indent_lines(&format!(
                        "case {} {}",
                        pats.join(", "),
                        self.render_block(&arm.body)
                    )));
                    s.push('\n');
                }
                if let Some(def) = &sw.default {
                    s.push_str(&indent_lines(&format!("default {}", self.render_block(def))));
                    s.push('\n');
                }
                s.push('}');
                s
            }
            ExprKind::Return(v) => match v {
                Some(v) => format!("return {}", self.expr(v)),
                None => "return".to_owned(),
            },
            ExprKind::Break { label, value } => {
                let mut s = "break".to_owned();
                if let Some(l) = label {
                    s.push(' ');
                    s.push_str(&l.name);
                }
                if let Some(v) = value {
                    s.push(' ');
                    s.push_str(&self.expr(v));
                }
                s
            }
            ExprKind::Continue { label } => match label {
                Some(l) => format!("continue {}", l.name),
                None => "continue".to_owned(),
            },
            ExprKind::Intrinsic { name, args } => {
                if args.is_empty() {
                    format!("#{}", name.name)
                } else {
                    let a: Vec<String> = args.iter().map(|x| self.expr(x)).collect();
                    format!("#{}({})", name.name, a.join(", "))
                }
            }
            ExprKind::MacroCall { name, body } => {
                format!("{}!{}", name.name, self.text(*body))
            }
            ExprKind::Tuple(elems) => {
                let e: Vec<String> = elems.iter().map(|x| self.expr(x)).collect();
                if e.len() == 1 {
                    format!("({},)", e[0])
                } else {
                    format!("({})", e.join(", "))
                }
            }
            ExprKind::Error => "/* <error> */".to_owned(),
        }
    }

    fn struct_body(&self, fields: &[FieldInit]) -> String {
        if fields.is_empty() {
            return "{}".to_owned();
        }
        let rendered: Vec<String> = fields
            .iter()
            .map(|f| match &f.name {
                Some(n) => format!("{}: {}", n.name, self.expr(&f.value)),
                None => self.expr(&f.value),
            })
            .collect();
        let inline = format!("{{ {} }}", rendered.join(", "));
        if inline.chars().count() + self.indent * INDENT.len() <= MAX_WIDTH
            && !inline.contains('\n')
        {
            inline
        } else {
            let mut s = "{\n".to_owned();
            for r in &rendered {
                s.push_str(&indent_lines(&format!("{r},")));
                s.push('\n');
            }
            s.push('}');
            s
        }
    }

    fn render_block(&self, b: &Block) -> String {
        if b.stmts.is_empty() {
            return "{}".to_owned();
        }
        // Nested blocks inside an expression are rendered by re-entering a sub-printer, so
        // statement layout is identical wherever a block appears.
        let mut sub = Printer {
            file: self.file,
            src: self.src,
            comments: &[],
            next_comment: 0,
            out: String::new(),
            indent: 0,
            pending_blank: false,
        };
        sub.block_body(b);
        let body = sub.out;
        let mut s = "{\n".to_owned();
        for l in body.trim_end_matches('\n').split('\n') {
            s.push_str(INDENT);
            s.push_str(l);
            s.push('\n');
        }
        s.push('}');
        s
    }

    fn closure_block(&self, binding: &Option<Ident>, body: &Block) -> String {
        let rendered = self.render_block(body);
        match binding {
            Some(b) => {
                // `{ it -> ...` — splice the binding in after the opening brace.
                if let Some(rest) = rendered.strip_prefix("{\n") {
                    format!("{{ {} ->\n{}", b.name, rest)
                } else if rendered == "{}" {
                    format!("{{ {} -> }}", b.name)
                } else {
                    rendered
                }
            }
            None => rendered,
        }
    }
}

// --- helpers ---------------------------------------------------------------

/// End of a run of consecutive fields with no blank line between them.
fn field_run_end(members: &[Member], start: usize, p: &Printer<'_>) -> usize {
    let mut i = start;
    while i < members.len() {
        let is_field = matches!(members[i].kind, MemberKind::Field(_));
        if !is_field {
            break;
        }
        // A blank line ends the alignment group, as does a doc comment or attribute.
        if i > start
            && (p.blank_line_before(members[i].span.start)
                || !members[i].docs.is_empty()
                || !members[i].attrs.is_empty())
        {
            break;
        }
        i += 1;
    }
    i
}

fn indent_lines(text: &str) -> String {
    text.split('\n')
        .map(|l| if l.is_empty() { l.to_owned() } else { format!("{INDENT}{l}") })
        .collect::<Vec<_>>()
        .join("\n")
}

fn vis_str(v: Vis) -> &'static str {
    match v {
        Vis::Private => "",
        Vis::Module => "pub(module) ",
        Vis::Public => "pub ",
    }
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
                let b: Vec<String> = p.bounds.iter().map(dump::path_str).collect();
                format!("{}: {}", p.name.name, b.join(" + "))
            }
        })
        .collect();
    format!("<{}>", params.join(", "))
}

fn traits_str(traits: &[Path]) -> String {
    if traits.is_empty() {
        String::new()
    } else {
        format!(" : {}", traits.iter().map(dump::path_str).collect::<Vec<_>>().join(" + "))
    }
}

/// Precedence of an expression's outermost operator, mirroring spec §11.1. Used to decide
/// where parentheses are required so re-parsing yields the same tree.
fn expr_precedence(e: &Expr) -> u8 {
    match &e.kind {
        ExprKind::Or { .. } => 1,
        ExprKind::Binary { op, .. } => binary_prec(*op).0,
        ExprKind::Range { .. } => 5,
        ExprKind::Return(_) | ExprKind::Break { .. } => 2,
        ExprKind::Cast { .. } => 12,
        ExprKind::Unary { .. } | ExprKind::Ref { .. } => 13,
        _ => 15,
    }
}

/// Minimum precedence required of each operand, so left-associativity is preserved.
fn binary_prec(op: BinaryOp) -> (u8, u8) {
    use BinaryOp::*;
    match op {
        Or => (2, 3),
        And => (3, 4),
        Eq | Ne | Lt | Gt | Le | Ge => (4, 5),
        BitOr => (6, 7),
        BitXor => (7, 8),
        BitAnd => (8, 9),
        Shl | Shr => (9, 10),
        Add | Sub => (10, 11),
        Mul | Div | Rem => (11, 12),
    }
}
