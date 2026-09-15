//! Editor queries: hover, go to definition, references and rename, completion, signature
//! help, inlay hints, the outline, workspace symbols, formatting.
//!
//! Everything here is a pure function of an [`Analysis`] (or of one file's text) and a
//! position, with no protocol types in sight, so each query is tested directly against a
//! real program rather than through a client.
//!
//! The typed tree does the heavy lifting. Every expression already knows its type and what
//! it names — a [`LocalId`], a [`SymbolId`], a field index — so "what is under the cursor"
//! is a search for the innermost expression there, not a second resolver (spec §17.1).
//! Names outside bodies (a type in a signature, an import) have no expression, and fall
//! back to the lookup name resolution itself uses, so the two cannot disagree.
//!
//! The server analyses in [`dz_driver::Mode::Editor`], so a tree, symbols and types exist
//! even while the buffer has a syntax error; the queries simply see less of the program.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use dz_ast::{
    CfgElse, CfgIf, Field, FnDecl, Ident, Item, ItemKind, Member, MemberKind, SourceFileAst, Type,
    TypeKind, Vis,
};
use dz_check::{type_name, type_name_in_file, FieldSig, FnSig, ItemSig, TypeTable, VariantSig};
use dz_diag::Diagnostics;
use dz_driver::Analysis;
use dz_hir::ty::Ty;
use dz_hir::{Body, Callee, ExprId, ExprKind, Hir, LocalId, LocalKind};
use dz_lex::{Comment, Token, TokenKind};
use dz_project::ModuleId;
use dz_resolve::{
    MemberSort, Resolution, SymbolId, SymbolKind, BUILTIN_FNS, BUILTIN_TYPES, TYPE_QUERIES,
};
use dz_span::{FileId, SourceMap, Span};

// ---------------------------------------------------------------------------
// one file on its own
// ---------------------------------------------------------------------------

/// One buffer, lexed and parsed without a project — enough for the outline and formatting,
/// which must work on files no manifest lists.
pub struct Parsed {
    pub map: SourceMap,
    pub file: FileId,
    pub ast: SourceFileAst,
    pub comments: Vec<Comment>,
    pub diagnostics: Diagnostics,
}

pub fn parse(path: &Path, text: &str) -> Parsed {
    let mut map = SourceMap::new();
    let file = map.add(path.to_owned(), text.to_owned());
    let lexed = dz_lex::lex(file, text);
    let parsed = dz_parse::parse(file, text, &lexed.tokens);
    let mut diagnostics = lexed.diagnostics;
    diagnostics.extend(parsed.diagnostics);
    diagnostics.sort_by_position();
    Parsed { map, file, ast: parsed.ast, comments: lexed.comments, diagnostics }
}

/// The formatted text, or `None` when the source has syntax errors: formatting it would drop
/// whatever did not parse (invariant 10).
pub fn format(parsed: &Parsed) -> Option<String> {
    if parsed.diagnostics.has_errors() {
        return None;
    }
    Some(dz_fmt::format(parsed.map.get(parsed.file), &parsed.ast, &parsed.comments))
}

/// A standard-library pseudo-path (`<std>/text/string.dz`) names nothing an editor can open or
/// change.
fn is_pseudo(path: &Path) -> bool {
    path.to_string_lossy().starts_with('<')
}

/// The embedded file at `pseudo` (`<std>/math/vec3.dz`) that the analysis parsed and checked, if
/// this build includes it. Looked up through the parsed files, not the source map: the map
/// may hold an embedded text more than once, and only one of those ids has a tree,
/// declarations and bodies.
pub fn embedded_file(a: &Analysis, pseudo: &str) -> Option<FileId> {
    a.files
        .values()
        .flatten()
        .map(|f| f.file)
        .find(|&f| a.map.get(f).path().to_string_lossy().replace('\\', "/") == pseudo)
}

/// The links in the manifest at `manifest`, in the manifest's file: a recipe's
/// `import = ["raylib.h"]` to the header the include search found, a `cpp = "raylib"` to the
/// same header, and a `path` to the dependency's manifest (spec §17.4). Each with a tooltip.
pub fn manifest_links(a: &Analysis, manifest: &Path) -> Vec<(FileId, Span, PathBuf, String)> {
    let Some(ws) = a.ws.as_ref() else { return Vec::new() };
    let same = |p: &Path| match (p.canonicalize(), manifest.canonicalize()) {
        (Ok(x), Ok(y)) => x == y,
        _ => p == manifest,
    };
    let header_of = |recipe: &str| {
        ws.modules().find(|m| m.name == recipe).and_then(|m| m.foreign.as_ref()).map(|f| &f.header)
    };
    let mut out = Vec::new();
    for m in ws.modules().filter(|m| m.foreign.is_none() && !m.embedded && same(&m.manifest_path)) {
        for d in m.deps.iter().filter(|d| !d.implicit && d.source_span != Span::DUMMY) {
            let target = ws.module(d.module);
            if target.embedded {
                continue;
            }
            match &target.foreign {
                Some(f) => out.push((
                    m.manifest,
                    d.source_span,
                    f.header.path.clone(),
                    format!("the header `{}`", f.header.requested.join("`, `")),
                )),
                None => out.push((
                    m.manifest,
                    d.source_span,
                    target.manifest_path.clone(),
                    format!("module `{}`", target.name),
                )),
            }
        }
        for lib in &m.cpp.libs {
            let Some(header) = header_of(&lib.name) else { continue };
            for (name, span) in &lib.import {
                // Each entry to the file the include search found for it.
                let Some(i) = header.requested.iter().position(|r| r == name) else { continue };
                let Some(path) = header.paths.get(i) else { continue };
                out.push((m.manifest, *span, path.clone(), format!("the header `{name}`")));
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// declarations: the outline, and what hover shows
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeclKind {
    Function,
    Method,
    Ctor,
    Struct,
    Union,
    Enum,
    Trait,
    Impl,
    Const,
    Macro,
    Field,
    Variant,
    TypeAlias,
}

/// A declaration as the user wrote it.
#[derive(Clone, Debug)]
pub struct Decl {
    pub name: String,
    pub kind: DeclKind,
    pub name_span: Span,
    pub span: Span,
    /// The declaration's source up to its body, on one line: `pub fn at(&self, i: usize) -> ^T`.
    /// Quoted from the source rather than rebuilt from types, so hover shows exactly what
    /// was written (principle 4, §1).
    pub header: String,
    /// The `///` comment above it, markers removed.
    pub docs: String,
    /// For an `impl`: the type it is for, and the trait it implements, by name.
    pub impl_for: Option<String>,
    pub impl_trait: Option<String>,
    pub children: Vec<Decl>,
}

impl Decl {
    fn new(kind: DeclKind, name: &Ident, span: Span, header: String, docs: String) -> Decl {
        Decl {
            name: name.name.clone(),
            kind,
            name_span: name.span,
            span,
            header,
            docs,
            impl_for: None,
            impl_trait: None,
            children: Vec::new(),
        }
    }
}

/// Every declaration in a file, members nested under their type.
pub fn decls(text: &str, ast: &SourceFileAst) -> Vec<Decl> {
    let mut items = Vec::new();
    for item in &ast.items {
        every_branch(
            item,
            &|i: &Item| match &i.kind {
                ItemKind::CfgIf(c) => Some(c),
                _ => None,
            },
            &mut items,
        );
    }
    items.into_iter().filter_map(|item| item_decl(text, item)).collect()
}

/// A declaration, or — for an `#if` — the declarations in every one of its branches, so the
/// outline shows what the file says whichever flags are set (spec §12.10).
fn every_branch<'a, T>(
    x: &'a T,
    cfg: &dyn Fn(&'a T) -> Option<&'a CfgIf<T>>,
    out: &mut Vec<&'a T>,
) {
    let Some(c) = cfg(x) else {
        out.push(x);
        return;
    };
    let mut next = Some(c);
    while let Some(c) = next {
        for y in &c.then {
            every_branch(y, cfg, out);
        }
        next = match &c.otherwise {
            Some(CfgElse::If(n)) => Some(n),
            Some(CfgElse::Else { items, .. }) => {
                for y in items {
                    every_branch(y, cfg, out);
                }
                None
            }
            None => None,
        };
    }
}

fn item_decl(text: &str, item: &Item) -> Option<Decl> {
    let docs = doc_text(text, &item.docs);
    let mut d = match &item.kind {
        // Expanded by `decls` before it gets here.
        ItemKind::CfgIf(_) => return None,
        ItemKind::Fn(f) => fn_decl(text, f, DeclKind::Function, docs),
        ItemKind::Struct(s) => {
            let header = braced_header(text, s.span, s.name.span);
            let mut d = Decl::new(DeclKind::Struct, &s.name, s.span, header, docs);
            d.children = members(text, &s.members);
            d
        }
        ItemKind::Union(u) => {
            let header = braced_header(text, u.span, u.name.span);
            let mut d = Decl::new(DeclKind::Union, &u.name, u.span, header, docs);
            d.children = u
                .variants
                .iter()
                .map(|v| {
                    let header = self::header(text, v.span.start, v.span.end);
                    let mut vd = Decl::new(
                        DeclKind::Variant,
                        &v.name,
                        v.span,
                        header,
                        doc_text(text, &v.docs),
                    );
                    vd.children =
                        v.fields.iter().map(|f| field_decl(text, f, String::new())).collect();
                    vd
                })
                .collect();
            d
        }
        ItemKind::Enum(e) => {
            let header = braced_header(text, e.span, e.name.span);
            let mut d = Decl::new(DeclKind::Enum, &e.name, e.span, header, docs);
            d.children = e
                .variants
                .iter()
                .map(|v| {
                    let header = self::header(text, v.span.start, v.span.end);
                    Decl::new(DeclKind::Variant, &v.name, v.span, header, doc_text(text, &v.docs))
                })
                .collect();
            d
        }
        ItemKind::Trait(t) => {
            let header = braced_header(text, t.span, t.name.span);
            let mut d = Decl::new(DeclKind::Trait, &t.name, t.span, header, docs);
            d.children = members(text, &t.items);
            d
        }
        ItemKind::Impl(i) => {
            let head = header(text, i.span.start, brace_after(text, i.span, i.self_ty.span.end));
            let trait_name =
                i.trait_path.as_ref().and_then(|p| p.segments.last()).map(|s| s.name.name.clone());
            Decl {
                name: head.clone(),
                kind: DeclKind::Impl,
                name_span: i.trait_path.as_ref().map_or(i.self_ty.span, |p| p.span),
                span: i.span,
                header: head,
                docs,
                impl_for: type_base(&i.self_ty),
                impl_trait: trait_name,
                children: members(text, &i.items),
            }
        }
        ItemKind::Const(c) => {
            let header = capped(header(text, c.span.start, c.span.end));
            Decl::new(DeclKind::Const, &c.name, c.span, header, docs)
        }
        ItemKind::Static(st) => {
            let header = capped(header(text, st.span.start, st.span.end));
            Decl::new(DeclKind::Const, &st.name, st.span, header, docs)
        }
        ItemKind::Macro(m) => {
            let header = header(text, m.span.start, m.body.start);
            Decl::new(DeclKind::Macro, &m.name, m.span, header, docs)
        }
        ItemKind::MacroInvocation { .. } | ItemKind::Error => return None,
    };
    with_vis(&mut d, item.vis);
    Some(d)
}

fn members(text: &str, members: &[Member]) -> Vec<Decl> {
    let mut all = Vec::new();
    for m in members {
        every_branch(
            m,
            &|x: &Member| match &x.kind {
                MemberKind::CfgIf(c) => Some(c),
                _ => None,
            },
            &mut all,
        );
    }
    all.into_iter()
        .filter_map(|m| {
            let docs = doc_text(text, &m.docs);
            let mut d = match &m.kind {
                // Expanded above.
                MemberKind::CfgIf(_) => return None,
                MemberKind::Field(f) => field_decl(text, f, docs),
                MemberKind::Fn(f) => fn_decl(text, f, DeclKind::Method, docs),
                MemberKind::Ctor(f) => fn_decl(text, f, DeclKind::Ctor, docs),
                MemberKind::TypeAlias { name, .. } => {
                    let header = header(text, m.span.start, m.span.end);
                    Decl::new(DeclKind::TypeAlias, name, m.span, header, docs)
                }
                MemberKind::Const(c) => {
                    let header = capped(header(text, c.span.start, c.span.end));
                    Decl::new(DeclKind::Const, &c.name, c.span, header, docs)
                }
                MemberKind::Error => return None,
            };
            with_vis(&mut d, m.vis);
            Some(d)
        })
        .collect()
}

fn fn_decl(text: &str, f: &FnDecl, kind: DeclKind, docs: String) -> Decl {
    let end = f.body.as_ref().map_or(f.span.end, |b| b.span.start);
    Decl::new(kind, &f.name, f.span, header(text, f.span.start, end), docs)
}

fn field_decl(text: &str, f: &Field, docs: String) -> Decl {
    Decl::new(DeclKind::Field, &f.name, f.span, header(text, f.span.start, f.span.end), docs)
}

fn with_vis(d: &mut Decl, vis: Vis) {
    let prefix = match vis {
        Vis::Private => return,
        Vis::Protected => "protected ",
        Vis::Module => "pub(module) ",
        Vis::Public => "pub ",
    };
    if !d.header.starts_with("pub") && !d.header.starts_with("protected") {
        d.header.insert_str(0, prefix);
    }
}

/// The source between two offsets, whitespace collapsed onto one line.
fn header(text: &str, start: u32, end: u32) -> String {
    let src = text.get(start as usize..end.max(start) as usize).unwrap_or("");
    let line = src.split_whitespace().collect::<Vec<_>>().join(" ");
    line.trim_end_matches('{').trim_end().to_owned()
}

/// A type-like declaration's header: everything up to the `{` that opens its body.
fn braced_header(text: &str, span: Span, name: Span) -> String {
    header(text, span.start, brace_after(text, span, name.end))
}

fn brace_after(text: &str, span: Span, from: u32) -> u32 {
    text.get(from as usize..span.end as usize)
        .and_then(|s| s.find('{'))
        .map_or(span.end, |i| from + i as u32)
}

/// A constant's initialiser can be arbitrarily long; hover wants the gist.
fn capped(s: String) -> String {
    const MAX: usize = 160;
    if s.chars().count() <= MAX {
        return s;
    }
    let mut out: String = s.chars().take(MAX).collect();
    out.push_str(" …");
    out
}

fn doc_text(text: &str, docs: &[Span]) -> String {
    docs.iter()
        .map(|s| {
            let line = text.get(s.range()).unwrap_or("");
            let line = line.strip_prefix("///").unwrap_or(line);
            line.strip_prefix(' ').unwrap_or(line).trim_end()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `Buffer` for `Buffer<T>`, `&Buffer<T>` and `^Buffer<T>`: what an `impl` is for, by name.
fn type_base(t: &Type) -> Option<String> {
    match &t.kind {
        TypeKind::Path(p) => p.segments.last().map(|s| s.name.name.clone()),
        TypeKind::Ptr { inner, .. }
        | TypeKind::OwnPtr { inner, .. }
        | TypeKind::Ref { inner, .. } => type_base(inner),
        _ => None,
    }
}

fn flatten(decls: &[Decl]) -> Vec<&Decl> {
    let mut out = Vec::new();
    fn walk<'d>(decls: &'d [Decl], out: &mut Vec<&'d Decl>) {
        for d in decls {
            out.push(d);
            walk(&d.children, out);
        }
    }
    walk(decls, &mut out);
    out
}

/// The declaration named at `span`, with the one it is nested in. An `impl` has no name of
/// its own — its `name_span` is the trait it implements — so it never matches.
fn named_with_parent(decls: &[Decl], span: Span) -> Option<(&Decl, Option<&Decl>)> {
    fn walk<'d>(
        decls: &'d [Decl],
        parent: Option<&'d Decl>,
        span: Span,
    ) -> Option<(&'d Decl, Option<&'d Decl>)> {
        for d in decls {
            if d.name_span == span && d.kind != DeclKind::Impl {
                return Some((d, parent));
            }
            if let Some(hit) = walk(&d.children, Some(d), span) {
                return Some(hit);
            }
        }
        None
    }
    walk(decls, None, span)
}

fn code(s: &str) -> String {
    format!("```dazzle\n{s}\n```")
}

fn render(d: &Decl) -> String {
    let mut s = code(&d.header);
    if !d.docs.is_empty() {
        s.push_str("\n\n---\n\n");
        s.push_str(&doxygen(&d.docs));
    }
    s
}

/// A documentation comment as Markdown, understanding Doxygen's commands.
///
/// Dazzle writes `///` and C and C++ headers write whatever they like, but both use Doxygen
/// to say what a parameter is for and what a function returns — and an editor showing
/// `@param width the window width` verbatim is showing markup, not documentation. The
/// commands below are the ones that carry meaning worth laying out; anything else is passed
/// through untouched, so no comment is ever made worse by being rendered.
///
/// Both spellings of every command are accepted (`@param` and `\param`), as Doxygen does.
pub fn doxygen(docs: &str) -> String {
    let mut body: Vec<String> = Vec::new();
    let mut params: Vec<(String, String)> = Vec::new();
    let mut returns: Vec<String> = Vec::new();
    // Which list the following indented continuation lines belong to.
    enum At {
        Body,
        Param,
        Return,
    }
    let mut at = At::Body;
    let mut in_code = false;

    for line in docs.lines() {
        let t = line.trim_start();
        if in_code {
            if matches!(command(t), Some(("endcode" | "endverbatim", _))) {
                in_code = false;
                body.push("```".to_owned());
            } else {
                body.push(line.to_owned());
            }
            continue;
        }
        let Some((cmd, rest)) = command(t) else {
            // A continuation of whatever command came last.
            match (&at, t.is_empty()) {
                (_, true) => {
                    at = At::Body;
                    body.push(String::new());
                }
                (At::Param, _) => {
                    if let Some(last) = params.last_mut() {
                        last.1.push(' ');
                        last.1.push_str(t);
                    }
                }
                (At::Return, _) => {
                    if let Some(last) = returns.last_mut() {
                        last.push(' ');
                        last.push_str(t);
                    }
                }
                (At::Body, _) => body.push(line.to_owned()),
            }
            continue;
        };
        match cmd {
            // `@param[in] name description` — the direction is Doxygen's own bookkeeping.
            "param" => {
                let rest = rest.trim_start_matches('[').trim_start();
                let rest = match rest.find(']') {
                    Some(i) => rest[i + 1..].trim_start(),
                    None => rest,
                };
                let (name, desc) = split_word(rest);
                params.push((name.to_owned(), desc.to_owned()));
                at = At::Param;
            }
            "return" | "returns" | "result" => {
                returns.push(rest.to_owned());
                at = At::Return;
            }
            "brief" | "short" | "details" | "summary" => {
                body.push(rest.to_owned());
                at = At::Body;
            }
            "note" | "warning" | "remark" | "remarks" | "attention" | "since" | "deprecated" => {
                let label = format!("{}{}", cmd[..1].to_uppercase(), &cmd[1..]);
                body.push(format!("**{label}:** {rest}"));
                at = At::Body;
            }
            "see" | "sa" => {
                body.push(format!("**See:** {rest}"));
                at = At::Body;
            }
            "code" | "verbatim" => {
                in_code = true;
                body.push("```".to_owned());
            }
            // `@p name`, `@c thing`, `@a arg`: this word is code.
            "p" | "c" | "a" | "e" | "em" => {
                let (word, tail) = split_word(rest);
                body.push(format!("`{word}` {tail}").trim_end().to_owned());
            }
            _ => body.push(line.to_owned()),
        }
    }
    if in_code {
        body.push("```".to_owned());
    }

    let mut out = body.join("\n").trim().to_owned();
    if !params.is_empty() {
        out.push_str("\n\n**Parameters**\n");
        for (name, desc) in &params {
            let desc = desc.trim();
            if desc.is_empty() {
                out.push_str(&format!("\n- `{name}`"));
            } else {
                out.push_str(&format!("\n- `{name}` — {desc}"));
            }
        }
    }
    if !returns.is_empty() {
        let text = returns.join(" ");
        out.push_str(&format!("\n\n**Returns** {}", text.trim()));
    }
    out.trim().to_owned()
}

/// A Doxygen command at the start of a line, in either spelling, and what follows it.
fn command(line: &str) -> Option<(&str, &str)> {
    let rest = line.strip_prefix('@').or_else(|| line.strip_prefix('\\'))?;
    let end = rest.find(|c: char| !c.is_ascii_alphabetic()).unwrap_or(rest.len());
    if end == 0 {
        return None;
    }
    Some((&rest[..end], rest[end..].trim_start()))
}

fn split_word(s: &str) -> (&str, &str) {
    match s.find(char::is_whitespace) {
        Some(i) => (&s[..i], s[i..].trim_start()),
        None => (s, ""),
    }
}

// ---------------------------------------------------------------------------
// what a name means
// ---------------------------------------------------------------------------

/// Hover and go-to-definition for one position.
pub struct Info {
    /// The name (or expression) the answer is about, to highlight.
    pub span: Span,
    pub definition: Option<(FileId, Span)>,
    /// Markdown.
    pub hover: Option<String>,
}

/// Something a name can refer to.
enum Ref<'a> {
    Local(&'a Body, LocalId),
    Symbol(SymbolId),
    /// A struct field, by declaration order.
    Field(SymbolId, usize),
    /// A field of a union variant's payload: union, variant, field.
    VariantField(SymbolId, usize, usize),
    /// A union or enum variant.
    Variant(SymbolId, usize),
    Ctor(SymbolId, usize),
    Method(SymbolId, String),
}

/// What the name under the cursor is: a declaration's own name, or a use of one.
enum Target<'a> {
    Decl(FileId, Decl),
    Ref(Ref<'a>),
}

enum Adt {
    Struct(SymbolId),
    Variant(SymbolId, usize),
}

/// The declaration a value's members come from, reaching through `&`, `^`, `own ^` and `?`
/// the way `.` does (spec §5.2).
fn adt_of(ty: &Ty) -> Option<Adt> {
    match ty {
        Ty::Named { sym, .. } => Some(Adt::Struct(*sym)),
        Ty::Variant(s, v, _) => Some(Adt::Variant(*s, *v)),
        Ty::Ref { inner, .. }
        | Ty::Ptr { inner, .. }
        | Ty::OwnPtr { inner, .. }
        | Ty::Optional(inner) => adt_of(inner),
        _ => None,
    }
}

fn peel(ty: &Ty) -> &Ty {
    match ty {
        Ty::Ref { inner, .. }
        | Ty::Ptr { inner, .. }
        | Ty::OwnPtr { inner, .. }
        | Ty::Optional(inner) => peel(inner),
        _ => ty,
    }
}

fn is_name(kind: TokenKind) -> bool {
    matches!(kind, TokenKind::Ident | TokenKind::SelfValue | TokenKind::SelfType)
}

fn name_token_at(tokens: &[Token], offset: u32) -> Option<usize> {
    tokens.iter().position(|t| is_name(t.kind) && t.span.start <= offset && offset <= t.span.end)
}

/// Hover and definition for the name (or expression) at `offset`.
pub fn info_at(a: &Analysis, file: FileId, offset: u32) -> Option<Info> {
    let q = Query::new(a)?;
    let text = a.map.get(file).text();
    let tokens = dz_lex::lex(file, text).tokens;
    let Some(i) = name_token_at(&tokens, offset) else {
        // Not on a name: say what type the expression here has.
        let (body, e) = q.exprs_at(file, offset).into_iter().next()?;
        let expr = body.expr(e);
        return Some(Info {
            span: expr.span,
            definition: None,
            hover: Some(code(&type_name_in_file(q.res, file, &expr.ty))),
        });
    };
    if let Some(info) = module_info(&q, file, &tokens, text, i) {
        return Some(info);
    }
    let Some(target) = q.target_at(file, &tokens, text, i) else {
        // A member of a type the language provides (`e.message` on an `Error`) has no
        // declaration to show or go to, but the checker still typed it.
        let ty = member_access(&q, file, &tokens, i)?;
        let name = tokens[i].text(text);
        return Some(Info {
            span: tokens[i].span,
            definition: None,
            hover: Some(code(&format!("{name}: {}", type_name_in_file(q.res, file, &ty)))),
        });
    };
    Some(Info {
        span: tokens[i].span,
        definition: q.target_definition(&target),
        hover: q.target_hover(&target),
    })
}

/// A dependency key, or a public dependency along a path — `rl` in `rl::DrawLine`, `units`
/// in `phys::units::Meters` — which has no declaration of its own: it goes to the header a
/// `cpp` dependency imports or the manifest of a project, and says which (spec §3.2, §17.4).
fn module_info(q: &Query, file: FileId, tokens: &[Token], text: &str, i: usize) -> Option<Info> {
    if tokens.get(i + 1).map(|t| t.kind) != Some(TokenKind::ColonColon) {
        return None;
    }
    let mut start = i;
    while start >= 2
        && tokens[start - 1].kind == TokenKind::ColonColon
        && is_name(tokens[start - 2].kind)
    {
        start -= 2;
    }
    let path: Vec<&str> = (start..=i).step_by(2).map(|j| tokens[j].text(text)).collect();
    let (m, used) = q.res.module_prefix(file, &path)?;
    if used != path.len() {
        return None;
    }
    let ws = q.a.ws.as_ref()?;
    let module = ws.module(m);
    let key = tokens[i].text(text);
    let (definition, what) = match &module.foreign {
        Some(f) => {
            // The file the first header was found at, among those the header module read.
            let first = f.header.paths.first();
            let id = f
                .files
                .iter()
                .copied()
                .find(|id| first.is_some_and(|p| q.a.map.get(*id).path() == p.as_path()))
                .or_else(|| f.files.first().copied());
            let names: Vec<String> = f.header.requested.iter().map(|h| format!("`{h}`")).collect();
            (
                id.map(|id| (id, Span::new(0, 0))),
                format!("the C or C++ headers {}", names.join(", ")),
            )
        }
        None if module.embedded => (None, "the standard library".to_owned()),
        None => (
            Some((module.manifest, Span::new(0, 0))),
            format!("module `{}`, `{}`", module.name, module.manifest_path.display()),
        ),
    };
    Some(Info {
        span: tokens[i].span,
        definition,
        hover: Some(format!("**dependency** `{key}` — {what}")),
    })
}

/// The type of `base.name` when `tokens[i]` is that `name`: the member access expression
/// the checker built, which starts at the base and ends at the name.
fn member_access(q: &Query, file: FileId, tokens: &[Token], i: usize) -> Option<Ty> {
    if i == 0 || tokens[i - 1].kind != TokenKind::Dot {
        return None;
    }
    let name = tokens[i].span;
    q.exprs_at(file, name.start)
        .into_iter()
        .map(|(b, e)| b.expr(e))
        .find(|e| e.span.end == name.end && e.span.start < name.start)
        .map(|e| e.ty.clone())
}

/// Every place the name at `offset` is written that means the same thing, across the project:
/// what references, highlighting and rename share.
///
/// A struct and its constructors are one thing — they share a name, and renaming one must
/// rename the other — and so are a trait method and the methods implementing it.
pub fn references(
    a: &Analysis,
    file: FileId,
    offset: u32,
    include_declaration: bool,
) -> Vec<(FileId, Span)> {
    let Some(q) = Query::new(a) else { return Vec::new() };
    let text = a.map.get(file).text();
    let tokens = dz_lex::lex(file, text).tokens;
    let Some(i) = name_token_at(&tokens, offset) else { return Vec::new() };
    let Some(key) = q.target_at(file, &tokens, text, i).and_then(|t| q.key(&t)) else {
        return Vec::new();
    };
    q.occurrences(key, tokens[i].text(text), include_declaration)
}

/// Why a rename cannot start here, or the name it would change.
pub fn prepare_rename(a: &Analysis, file: FileId, offset: u32) -> Result<Span, String> {
    let q = Query::new(a).ok_or("the project could not be analysed")?;
    let text = a.map.get(file).text();
    let tokens = dz_lex::lex(file, text).tokens;
    let i = name_token_at(&tokens, offset).ok_or("there is no name here to rename")?;
    let tok = tokens[i];
    if tok.kind != TokenKind::Ident {
        return Err("`self` and `Self` are keywords, not names".to_owned());
    }
    let name = tok.text(text);
    let key = q
        .target_at(file, &tokens, text, i)
        .and_then(|t| q.key(&t))
        .ok_or_else(|| format!("`{name}` does not name anything declared in this project"))?;
    if is_pseudo(a.map.get(key.0).path()) {
        return Err(format!("`{name}` is declared by the prelude, which cannot be edited"));
    }
    Ok(tok.span)
}

/// Every span to replace with `new_name`.
pub fn rename(
    a: &Analysis,
    file: FileId,
    offset: u32,
    new_name: &str,
) -> Result<Vec<(FileId, Span)>, String> {
    prepare_rename(a, file, offset)?;
    let mut chars = new_name.chars();
    let starts_well = chars.next().is_some_and(|c| c == '_' || c.is_alphabetic());
    if !starts_well || !chars.all(is_ident_char) {
        return Err(format!("`{new_name}` is not a name"));
    }
    if dz_lex::keyword(new_name).is_some() || dz_lex::RESERVED.contains(&new_name) {
        return Err(format!("`{new_name}` is a keyword"));
    }
    Ok(references(a, file, offset, true))
}

struct Query<'a> {
    a: &'a Analysis,
    res: &'a Resolution,
    hir: Option<&'a Hir>,
    table: Option<&'a TypeTable>,
    decls: RefCell<HashMap<FileId, Rc<Vec<Decl>>>>,
}

impl<'a> Query<'a> {
    fn new(a: &'a Analysis) -> Option<Query<'a>> {
        Some(Query {
            a,
            res: a.resolution.as_ref()?,
            hir: a.hir.as_ref(),
            table: a.table.as_ref(),
            decls: RefCell::default(),
        })
    }

    fn decls(&self, file: FileId) -> Rc<Vec<Decl>> {
        if let Some(d) = self.decls.borrow().get(&file) {
            return d.clone();
        }
        let text = self.a.map.get(file).text();
        let found = self.a.files.values().flatten().find(|f| f.file == file);
        let d = Rc::new(found.map(|f| decls(text, &f.ast)).unwrap_or_default());
        self.decls.borrow_mut().insert(file, d.clone());
        d
    }

    fn all_files(&self) -> Vec<FileId> {
        let mut files: Vec<FileId> = self.a.files.values().flatten().map(|f| f.file).collect();
        files.sort();
        files
    }

    fn module_of(&self, file: FileId) -> Option<ModuleId> {
        self.a.files.iter().find(|(_, fs)| fs.iter().any(|f| f.file == file)).map(|(m, _)| *m)
    }

    /// The declaration named at `span`, or failing that the innermost one around it.
    fn decl_at(&self, file: FileId, span: Span) -> Option<Decl> {
        let decls = self.decls(file);
        let all = flatten(&decls);
        if let Some(d) = all.iter().find(|d| d.name_span == span) {
            return Some((*d).clone());
        }
        all.into_iter()
            .filter(|d| d.span.start <= span.start && span.end <= d.span.end)
            .min_by_key(|d| d.span.len())
            .cloned()
    }

    /// The name span of the declaration called `name` around `span`. The checker records
    /// some declarations by their whole extent (a field with its type and default); the
    /// editor wants the name, and references need one spelling per declaration.
    fn name_span(&self, file: FileId, span: Span, name: &str) -> Span {
        let decls = self.decls(file);
        flatten(&decls)
            .into_iter()
            .filter(|d| d.name == name && d.span.start <= span.start && span.end <= d.span.end)
            .min_by_key(|d| d.span.len())
            .map_or(span, |d| d.name_span)
    }

    fn fields(&self, s: SymbolId) -> Option<&'a [FieldSig]> {
        self.table?.struct_sig(s).map(|sig| sig.fields.as_slice())
    }

    fn variants(&self, s: SymbolId) -> Option<&'a [VariantSig]> {
        match self.table?.get(s)? {
            ItemSig::Union { variants } | ItemSig::Enum { variants, .. } => Some(variants),
            _ => None,
        }
    }

    /// Every typed expression in `file` containing `offset`, innermost first.
    fn exprs_at(&self, file: FileId, offset: u32) -> Vec<(&'a Body, ExprId)> {
        let Some(hir) = self.hir else { return Vec::new() };
        let mut found = Vec::new();
        for (_, body) in hir.bodies().filter(|(_, b)| b.file == file) {
            for (i, e) in body.exprs.iter().enumerate() {
                let inside = e.span.start <= offset && offset <= e.span.end;
                if inside && !e.span.is_empty() && !matches!(e.kind, ExprKind::Error) {
                    found.push((body, ExprId(i as u32)));
                }
            }
        }
        found.sort_by_key(|(b, e)| b.expr(*e).span.len());
        found
    }

    /// The struct a method body belongs to, whose fields are in scope bare (spec §7.3).
    fn self_type(&self, body: &Body) -> Option<SymbolId> {
        let local = body.locals.iter().find(|l| l.name == "self")?;
        match adt_of(&local.ty)? {
            Adt::Struct(s) => Some(s),
            Adt::Variant(..) => None,
        }
    }

    /// [`adt_of`], with `Self` standing for the type whose body this is.
    fn adt_in(&self, body: &Body, ty: &Ty) -> Option<Adt> {
        adt_of(ty).or_else(|| match peel(ty) {
            Ty::SelfTy => self.self_type(body).map(Adt::Struct),
            _ => None,
        })
    }

    /// What an expression names, most specific first.
    fn expr_refs(&self, body: &'a Body, e: ExprId) -> Vec<Ref<'a>> {
        match &body.expr(e).kind {
            ExprKind::Local(l) => vec![Ref::Local(body, *l)],
            ExprKind::Const(s) | ExprKind::Static(s) | ExprKind::FnItem { sym: s, .. } => {
                vec![Ref::Symbol(*s)]
            }
            ExprKind::Field { base, index } => match self.adt_in(body, body.ty(*base)) {
                Some(Adt::Struct(s)) => vec![Ref::Field(s, *index)],
                Some(Adt::Variant(s, v)) => vec![Ref::VariantField(s, v, *index)],
                None => Vec::new(),
            },
            ExprKind::StructLit { ty, fields } => {
                let mut refs = vec![Ref::Symbol(*ty)];
                refs.extend((0..fields.len()).map(|i| Ref::Field(*ty, i)));
                refs
            }
            ExprKind::EnumVariant { owner, index } => vec![Ref::Variant(*owner, *index)],
            ExprKind::AssocConst { owner, name } => vec![Ref::Method(*owner, name.clone())],
            ExprKind::UnionVariant { owner, index, payload } => {
                let mut refs = vec![Ref::Variant(*owner, *index)];
                refs.extend((0..payload.len()).map(|i| Ref::VariantField(*owner, *index, i)));
                refs
            }
            ExprKind::Call { callee, .. } => match callee {
                Callee::Fn { sym, .. } => vec![Ref::Symbol(*sym)],
                Callee::Method { owner, name, .. }
                // `UiElement::draw(self)` names the base's method, and that is the one
                // hover, definition and references should all land on (spec §7.3).
                | Callee::BaseMethod { owner, name, .. }
                | Callee::Assoc { owner, name, .. } => {
                    vec![Ref::Method(*owner, name.clone())]
                }
                // The constructor, or the type when it has none written.
                Callee::Ctor { owner, index } => vec![Ref::Ctor(*owner, *index), Ref::Symbol(*owner)],
                Callee::Value(_) => Vec::new(),
            },
            _ => Vec::new(),
        }
    }

    /// What the name token `tokens[i]` means.
    fn target_at(
        &self,
        file: FileId,
        tokens: &[Token],
        text: &str,
        i: usize,
    ) -> Option<Target<'a>> {
        let tok = tokens[i];
        let name = tok.text(text);
        let offset = tok.span.start;

        // A declaration's own name.
        let here = self.decls(file);
        if let Some((d, _)) = named_with_parent(&here, tok.span) {
            return Some(Target::Decl(file, d.clone()));
        }

        // A local where it is bound.
        if let Some(hir) = self.hir {
            for (_, body) in hir.bodies().filter(|(_, b)| b.file == file) {
                for (li, l) in body.locals.iter().enumerate() {
                    if l.name == name && l.span.start <= offset && offset <= l.span.end {
                        return Some(Target::Ref(Ref::Local(body, LocalId(li as u32))));
                    }
                }
            }
        }

        // A name inside checked code. The innermost expression containing the cursor is not
        // always the one the name belongs to — in `Buffer<Tracked>(3)` it is the constructor
        // call around `Tracked` — so an expression only answers for a name it actually uses.
        for (body, e) in self.exprs_at(file, offset) {
            for r in self.expr_refs(body, e) {
                if self.name(&r).as_deref() == Some(name) && self.definition(&r).is_some() {
                    return Some(Target::Ref(r));
                }
            }
        }

        // A name written where there is no expression: a type, an import, a `::` path.
        self.path_ref(file, tokens, i, text)
            .filter(|r| self.definition(r).is_some())
            .map(Target::Ref)
    }

    fn target_definition(&self, t: &Target<'a>) -> Option<(FileId, Span)> {
        match t {
            Target::Decl(file, d) => Some((*file, d.name_span)),
            Target::Ref(r) => self.definition(r),
        }
    }

    fn target_hover(&self, t: &Target<'a>) -> Option<String> {
        match t {
            Target::Decl(_, d) => Some(render(d)),
            Target::Ref(r) => self.hover(r),
        }
    }

    /// One identity per thing a name can mean: its declaration, with a constructor standing
    /// for its struct and an implementing method for the trait method it implements.
    fn key(&self, t: &Target<'a>) -> Option<(FileId, Span)> {
        let (file, span) = match t {
            Target::Ref(Ref::Ctor(s, _)) => {
                let sym = self.res.symbol(*s);
                (sym.file, sym.span)
            }
            _ => self.target_definition(t)?,
        };
        let decls = self.decls(file);
        let Some((d, parent)) = named_with_parent(&decls, span) else { return Some((file, span)) };
        match (d.kind, parent) {
            (DeclKind::Ctor, Some(p)) => Some((file, p.name_span)),
            (DeclKind::Method, Some(p)) if p.kind == DeclKind::Impl => {
                let Some(trait_name) = &p.impl_trait else { return Some((file, span)) };
                for f in self.all_files() {
                    for td in self.decls(f).iter() {
                        if td.kind == DeclKind::Trait && &td.name == trait_name {
                            if let Some(m) = td.children.iter().find(|c| c.name == d.name) {
                                return Some((f, m.name_span));
                            }
                        }
                    }
                }
                Some((file, span))
            }
            _ => Some((file, span)),
        }
    }

    fn occurrences(
        &self,
        key: (FileId, Span),
        name: &str,
        include_declaration: bool,
    ) -> Vec<(FileId, Span)> {
        let mut out = Vec::new();
        for f in self.all_files() {
            let source = self.a.map.get(f);
            // The prelude is only searched for its own names.
            if is_pseudo(source.path()) && f != key.0 {
                continue;
            }
            let text = source.text();
            let tokens = dz_lex::lex(f, text).tokens;
            for (j, t) in tokens.iter().enumerate() {
                if !is_name(t.kind) || t.text(text) != name {
                    continue;
                }
                let same =
                    self.target_at(f, &tokens, text, j).and_then(|t| self.key(&t)) == Some(key);
                if same && (include_declaration || (f, t.span) != key) {
                    out.push((f, t.span));
                }
            }
        }
        out
    }

    /// A name found only as a token: `Foo`, `alias::Foo`, `Foo::member`.
    fn path_ref(&self, file: FileId, tokens: &[Token], i: usize, text: &str) -> Option<Ref<'a>> {
        let module = self.module_of(file)?;
        let name = tokens[i].text(text);
        if i >= 2 && tokens[i - 1].kind == TokenKind::ColonColon && is_name(tokens[i - 2].kind) {
            // Every segment before this one, so `rl::shapes::Rectangle` walks the public
            // dependency `shapes` of `rl` (spec §3.2).
            let mut start = i - 2;
            while start >= 2
                && tokens[start - 1].kind == TokenKind::ColonColon
                && is_name(tokens[start - 2].kind)
            {
                start -= 2;
            }
            let before: Vec<&str> =
                (start..i - 1).step_by(2).map(|j| tokens[j].text(text)).collect();
            if let Some((m, used)) = self.res.module_prefix(file, &before) {
                if used == before.len() {
                    return self.res.lookup(m, name).map(Ref::Symbol);
                }
            }
            // `rl::Color::Red`, `Layer::Ui`: whatever the segments before name, then its member —
            // or, inside a C++ namespace, the declaration it holds.
            let (owner, used) = self.res.path_item(module, file, &before)?;
            if used != before.len() {
                return None;
            }
            if let Some(inner) = self.res.inner(owner, name) {
                return Some(Ref::Symbol(inner));
            }
            return Some(self.member_ref(owner, name));
        }
        // A `.member` the checker did not resolve has no answer a name lookup could give.
        if i >= 1 && tokens[i - 1].kind == TokenKind::Dot {
            return None;
        }
        self.res.lookup_in_file(module, file, name).map(Ref::Symbol)
    }

    fn member_ref(&self, owner: SymbolId, name: &str) -> Ref<'a> {
        if let Some(i) = self.variants(owner).and_then(|vs| vs.iter().position(|v| v.name == name))
        {
            return Ref::Variant(owner, i);
        }
        if let Some(i) = self.fields(owner).and_then(|fs| fs.iter().position(|f| f.name == name)) {
            return Ref::Field(owner, i);
        }
        Ref::Method(owner, name.to_owned())
    }

    fn name(&self, r: &Ref<'a>) -> Option<String> {
        Some(match r {
            Ref::Local(b, l) => b.local(*l).name.clone(),
            Ref::Symbol(s) | Ref::Ctor(s, _) => self.res.symbol(*s).name.clone(),
            Ref::Field(s, i) => self.fields(*s)?.get(*i)?.name.clone(),
            Ref::VariantField(s, v, i) => self.variants(*s)?.get(*v)?.fields.get(*i)?.name.clone(),
            Ref::Variant(s, v) => self.variants(*s)?.get(*v)?.name.clone(),
            Ref::Method(_, n) => n.clone(),
        })
    }

    fn definition(&self, r: &Ref<'a>) -> Option<(FileId, Span)> {
        let file_of = |s: &SymbolId| self.res.symbol(*s).file;
        let (file, span) = match r {
            Ref::Local(b, l) => return Some((b.file, b.local(*l).span)),
            Ref::Symbol(s) => (file_of(s), self.res.symbol(*s).span),
            Ref::Field(s, i) => (file_of(s), self.fields(*s)?.get(*i)?.span),
            Ref::VariantField(s, v, i) => {
                (file_of(s), self.variants(*s)?.get(*v)?.fields.get(*i)?.span)
            }
            Ref::Variant(s, v) => (file_of(s), self.variants(*s)?.get(*v)?.span),
            Ref::Ctor(s, i) => (file_of(s), self.table?.struct_sig(*s)?.ctors.get(*i)?.span),
            Ref::Method(s, n) => {
                let found = self.methods_of(*s).into_iter().find(|(_, d)| &d.name == n);
                let (file, d) = found.or_else(|| self.assoc_const(*s, n))?;
                return Some((file, d.name_span));
            }
        };
        Some((file, self.name_span(file, span, &self.name(r)?)))
    }

    fn hover(&self, r: &Ref<'a>) -> Option<String> {
        if let Ref::Local(b, l) = r {
            let local = b.local(*l);
            let ty = type_name(self.res, &local.ty);
            return Some(code(&match local.kind {
                LocalKind::Param => format!("{}: {ty}", local.name),
                _ if local.mutable => format!("let mut {}: {ty}", local.name),
                _ => format!("let {}: {ty}", local.name),
            }));
        }
        // An imported declaration is not written in Dazzle anywhere, so there is no source
        // to quote: its signature is rebuilt from the types the checker gave it, and its
        // documentation comes from the header (spec §15.2).
        if let Ref::Symbol(s) = r {
            if let Some(h) = self.foreign_hover(*s) {
                return Some(h);
            }
        }
        let (file, span) = self.definition(r)?;
        self.decl_at(file, span).map(|d| render(&d))
    }

    /// What an imported C or C++ name is, as Dazzle sees it, with the header's own comment.
    fn foreign_hover(&self, sym: SymbolId) -> Option<String> {
        let (head, docs, cpp, module) = self.foreign_decl(sym)?;
        // Where it came from, since the file it is "declared" in is a C header.
        let mut out = code(&format!("{head}\n// {cpp}, from {module}"));
        if !docs.is_empty() {
            out.push_str("\n\n---\n\n");
            out.push_str(&doxygen(&docs));
        }
        Some(out)
    }

    /// An imported declaration written the way Dazzle would write it, its documentation, its
    /// C++ path, and the header it came from.
    ///
    /// There is no Dazzle source to quote — the declaration is in a C or C++ header — so the
    /// signature is rebuilt from the types the checker gave it. Hover and signature help both
    /// need it, and must agree (spec §15.2).
    fn foreign_decl(&self, sym: SymbolId) -> Option<(String, String, String, String)> {
        let cpp = self.res.cpp_path(sym)?.to_owned();
        let symbol = self.res.symbol(sym);
        let module = self.a.ws.as_ref()?.module(symbol.module);
        let decl = module.foreign.as_ref()?.header.decls.iter().find(|d| d.cpp == cpp)?;

        let head = match self.table.and_then(|t| t.get(sym)) {
            Some(ItemSig::Fn(sig)) => {
                let params: Vec<String> = sig
                    .params
                    .iter()
                    .map(|p| format!("{}: {}", p.name, type_name(self.res, &p.ty)))
                    .collect();
                let ret = type_name(self.res, &sig.ret);
                let arrow = if ret == "void" { String::new() } else { format!(" -> {ret}") };
                format!("fn {}({}){arrow}", symbol.name, params.join(", "))
            }
            Some(ItemSig::Const(ty)) => {
                format!("const {}: {}", symbol.name, type_name(self.res, ty))
            }
            Some(ItemSig::Static(ty)) => {
                format!("static {}: {}", symbol.name, type_name(self.res, ty))
            }
            Some(ItemSig::Enum { variants, .. }) => {
                let names: Vec<&str> = variants.iter().map(|v| v.name.as_str()).collect();
                format!("enum {} {{ {} }}", symbol.name, names.join(", "))
            }
            Some(ItemSig::Struct(s)) if s.opaque => {
                format!("struct {}   // a handle: used behind ^T", symbol.name)
            }
            Some(ItemSig::Struct(s)) => {
                let fields: Vec<String> = s
                    .fields
                    .iter()
                    .map(|f| format!("    {}: {},", f.name, type_name(self.res, &f.ty)))
                    .collect();
                let word = if s.is_union { "union" } else { "struct" };
                if fields.is_empty() {
                    format!("{word} {}", symbol.name)
                } else {
                    format!("{word} {} {{\n{}\n}}", symbol.name, fields.join("\n"))
                }
            }
            _ => symbol.name.clone(),
        };
        Some((head, decl.docs.clone(), cpp, module.name.clone()))
    }

    /// The methods a value of this type can call, as declarations: those written in its own
    /// body (or a trait's), those in its `impl`s, then defaults inherited from the traits
    /// it implements (spec §7.6). Matched by name, which is exact within one program until
    /// two modules declare types of the same name; using the checker's own record of each
    /// implementation's target is owed by M9 (deferral register).
    /// `Allocators::Temp`: a constant declared in a type (spec §7.3), reached only through
    /// the type's name, so it is not among the members a value's `.` offers.
    fn assoc_const(&self, owner: SymbolId, name: &str) -> Option<(FileId, Decl)> {
        let sym = self.res.symbol(owner);
        let own = self.decls(sym.file);
        let d = flatten(&own).into_iter().find(|d| d.name_span == sym.span)?;
        let c = d.children.iter().find(|c| c.kind == DeclKind::Const && c.name == name)?;
        Some((sym.file, c.clone()))
    }

    fn methods_of(&self, owner: SymbolId) -> Vec<(FileId, Decl)> {
        let sym = self.res.symbol(owner);
        let mut out = Vec::new();
        let own = self.decls(sym.file);
        if let Some(d) = flatten(&own).into_iter().find(|d| d.name_span == sym.span) {
            out.extend(
                d.children
                    .iter()
                    .filter(|c| c.kind == DeclKind::Method)
                    .map(|c| (sym.file, c.clone())),
            );
        }
        let files = self.all_files();
        let mut traits = Vec::new();
        for &f in &files {
            for d in self.decls(f).iter() {
                if d.kind == DeclKind::Impl && d.impl_for.as_deref() == Some(sym.name.as_str()) {
                    out.extend(
                        d.children
                            .iter()
                            .filter(|c| c.kind == DeclKind::Method)
                            .map(|c| (f, c.clone())),
                    );
                    traits.extend(d.impl_trait.clone());
                }
            }
        }
        for &f in &files {
            for d in self.decls(f).iter() {
                if d.kind == DeclKind::Trait && traits.contains(&d.name) {
                    out.extend(
                        d.children
                            .iter()
                            .filter(|c| c.kind == DeclKind::Method)
                            .map(|c| (f, c.clone())),
                    );
                }
            }
        }
        out
    }

    // --- completion ------------------------------------------------------------------

    /// The innermost body whose block contains `offset`.
    fn body_at(&self, file: FileId, offset: u32) -> Option<&'a Body> {
        self.hir?
            .bodies()
            .map(|(_, b)| b)
            .filter(|b| b.file == file)
            .filter(|b| {
                let s = b.block(b.root).span;
                s.start <= offset && offset <= s.end
            })
            .min_by_key(|b| b.block(b.root).span.len())
    }

    /// The receiver of a `.` at byte `dot`: the widest expression that ends right there.
    /// This is the half-written line itself, checked, so `a.b().` is as easy as `a.`.
    fn receiver(&self, file: FileId, dot: u32) -> Option<(&'a Body, Ty)> {
        let hir = self.hir?;
        hir.bodies()
            .map(|(_, b)| b)
            .filter(|b| b.file == file)
            .flat_map(|b| b.exprs.iter().map(move |e| (b, e)))
            .filter(|(_, e)| {
                e.span.end == dot && !e.span.is_empty() && !matches!(e.kind, ExprKind::Error)
            })
            .max_by_key(|(_, e)| e.span.len())
            .map(|(b, e)| (b, e.ty.clone()))
    }

    fn symbol_completion(&self, s: SymbolId, out: &mut Completions) {
        let (kind, detail) = self.symbol_kind_detail(s);
        out.push(&self.res.symbol(s).name, kind, detail);
    }

    fn symbol_kind_detail(&self, s: SymbolId) -> (CompletionKind, Option<String>) {
        let sym = self.res.symbol(s);
        let kind = match sym.kind {
            SymbolKind::Struct => CompletionKind::Struct,
            SymbolKind::Union | SymbolKind::Enum => CompletionKind::Enum,
            SymbolKind::Trait => CompletionKind::Trait,
            SymbolKind::Fn => CompletionKind::Function,
            SymbolKind::Const => CompletionKind::Constant,
            SymbolKind::Static => CompletionKind::Variable,
            SymbolKind::Macro => CompletionKind::Macro,
            // A C++ namespace is a scope to reach through, which is what a module is here.
            SymbolKind::Namespace => CompletionKind::Module,
        };
        let detail = self.decl_at(sym.file, sym.span).map(|d| d.header);
        (kind, detail)
    }

    fn signature(&self, name: &str, f: &FnSig) -> String {
        let params = f
            .params
            .iter()
            .map(|p| format!("{}: {}", p.name, type_name(self.res, &p.ty)))
            .collect::<Vec<_>>()
            .join(", ");
        if f.ret == Ty::Void && !f.fallible {
            return format!("fn {name}({params})");
        }
        let arrow = if f.fallible { "!->" } else { "->" };
        format!("fn {name}({params}) {arrow} {}", type_name(self.res, &f.ret))
    }

    /// Fields and methods of a struct; its private ones only from inside it (spec §4).
    fn type_members(&self, owner: SymbolId, private: bool, out: &mut Completions) {
        let Some(sig) = self.table.and_then(|t| t.struct_sig(owner)) else { return };
        for f in &sig.fields {
            if private || f.vis != Vis::Private {
                out.push(&f.name, CompletionKind::Field, Some(type_name(self.res, &f.ty)));
            }
        }
        for key in &sig.method_keys {
            let Some(m) = sig.methods.get(key) else { continue };
            if private || m.vis != Vis::Private {
                let name = ident_prefix(key);
                out.push(name, CompletionKind::Method, Some(self.signature(name, m)));
            }
        }
    }

    /// Everything reachable with `.` on a value of type `ty`, seen from inside `body`.
    fn members_of(&self, body: &Body, ty: &Ty, out: &mut Completions) {
        let owner = self.self_type(body);
        match self.adt_in(body, ty) {
            Some(Adt::Struct(s)) => {
                self.type_members(s, owner == Some(s), out);
                for (_, d) in self.methods_of(s) {
                    out.push(&d.name, CompletionKind::Method, Some(d.header));
                }
            }
            Some(Adt::Variant(s, v)) => {
                if let Some(variant) = self.variants(s).and_then(|vs| vs.get(v)) {
                    for f in &variant.fields {
                        out.push(&f.name, CompletionKind::Field, Some(type_name(self.res, &f.ty)));
                    }
                }
            }
            None => match peel(ty) {
                // A generic parameter can do what its bounds say (spec §8.1).
                Ty::Param(p) => {
                    let bounds = self.table.map(|t| t.bounds_of(p)).unwrap_or_default();
                    for b in bounds {
                        for (_, d) in self.methods_of(b.trait_sym) {
                            out.push(&d.name, CompletionKind::Method, Some(d.header));
                        }
                    }
                }
                Ty::Array { .. } | Ty::Slice { .. } => {
                    out.push("len", CompletionKind::Field, Some("usize".to_owned()));
                    out.push("ptr", CompletionKind::Field, None);
                }
                _ => {}
            },
        }
    }

    /// After `recv.`, finding `recv` by name — for when the text being completed is not the
    /// text this analysis saw. Says whether it found the receiver.
    fn members_by_name(
        &self,
        file: FileId,
        recv: &str,
        offset: u32,
        out: &mut Completions,
    ) -> bool {
        let Some(body) = self.body_at(file, offset) else { return false };
        let local = body.locals.iter().rev().find(|l| l.name == recv && l.span.start <= offset);
        let ty = match local {
            Some(l) => l.ty.clone(),
            None => {
                let owner = self.self_type(body);
                let field = owner.and_then(|o| self.fields(o)?.iter().find(|f| f.name == recv));
                match field {
                    Some(f) => f.ty.clone(),
                    None => return false,
                }
            }
        };
        self.members_of(body, &ty, out);
        true
    }

    fn complete_scope(&self, file: FileId, offset: u32, out: &mut Completions) {
        let Some(module) = self.module_of(file) else { return };
        if let Some(body) = self.body_at(file, offset) {
            // Latest first, so a shadowing `let` wins the one slot per name.
            for l in body.locals.iter().rev().filter(|l| l.span.start <= offset) {
                out.push(&l.name, CompletionKind::Variable, Some(type_name(self.res, &l.ty)));
            }
            if let Some(owner) = self.self_type(body) {
                self.type_members(owner, true, out);
            }
        }
        // In the order a bare name is looked for (spec §3.2), so the one it means takes the
        // single slot a label gets.
        if let Some(scope) = self.res.scopes.get(&module) {
            for &s in scope.items.values() {
                self.symbol_completion(s, out);
            }
        }
        if let Some(imports) = self.res.imports.get(&file) {
            for &s in imports.unqualified.values() {
                self.symbol_completion(s, out);
            }
            for alias in imports.module_aliases.keys() {
                out.push(alias, CompletionKind::Module, None);
            }
        }
        let Some(names) = self.res.names.get(&module) else { return };
        for &s in names.picks.values() {
            self.symbol_completion(s, out);
        }
        for key in names.deps.keys() {
            out.push(key, CompletionKind::Module, None);
        }
        for m in &names.opened {
            for s in self.exports(*m, module) {
                // A name two opened dependencies export means neither; offering it would
                // offer an error (DZ0310).
                let name = &self.res.symbol(s).name;
                if matches!(
                    self.res.find_opened(module, &names.opened, name),
                    dz_resolve::Opened::One(_)
                ) {
                    self.symbol_completion(s, out);
                }
            }
        }
    }

    /// What `module` exports that `viewer` may name, in a stable order.
    fn exports(&self, module: ModuleId, viewer: ModuleId) -> Vec<SymbolId> {
        let Some(scope) = self.res.scopes.get(&module) else { return Vec::new() };
        let mut out: Vec<SymbolId> = scope
            .items
            .values()
            .copied()
            .filter(|s| {
                let sym = self.res.symbol(*s);
                dz_resolve::visible_from(sym.vis, sym.module, viewer)
            })
            .collect();
        out.sort();
        out.dedup();
        out
    }

    /// The names of dependencies the manifest does not open, matching `typed`, each to be
    /// written with its key: typing `DrawLi` offers `rl::DrawLine` (spec §3.2, §17.4). A
    /// name already in scope bare is not offered again.
    fn complete_through_keys(&self, file: FileId, typed: &str, out: &mut Completions) {
        let Some(module) = self.module_of(file) else { return };
        let Some(names) = self.res.names.get(&module) else { return };
        let mut keys: Vec<(&String, &ModuleId)> = names.deps.iter().collect();
        keys.sort();
        for (key, m) in keys {
            let opened = names.opened.contains(m);
            for s in self.exports(*m, module) {
                let sym = self.res.symbol(s);
                if !fuzzy_match(typed, &sym.name) || sym.kind == SymbolKind::Namespace {
                    continue;
                }
                // An opened dependency's name is offered bare already — unless another opened
                // one exports it too, when only the qualified spellings compile (DZ0310).
                if opened
                    && !matches!(
                        self.res.find_opened(module, &names.opened, &sym.name),
                        dz_resolve::Opened::Ambiguous(_)
                    )
                {
                    continue;
                }
                let (kind, detail) = self.symbol_kind_detail(s);
                out.push_qualified(key, &sym.name.clone(), kind, detail);
            }
        }
    }

    /// After `head::` — a module's items, or a type's members and compile-time queries.
    /// What can follow a `::`, given every segment written before it.
    ///
    /// The whole prefix, not just the last segment: `rl::MouseButton::` has to find
    /// `MouseButton` in the module `rl` names rather than in this file's own scope, and
    /// `shp::shapes::geom::` walks a C++ namespace per segment (spec §15.2).
    fn complete_path(&self, file: FileId, segments: &[&str], out: &mut Completions) {
        let Some(module) = self.module_of(file) else { return };
        let Some(head) = segments.first() else { return };

        // Modules first: a dependency key, then its public dependencies (spec §3.2).
        let (sym, rest) = match self.res.module_prefix(file, segments) {
            Some((m, used)) if used == segments.len() => {
                // `rl::` — everything the module exports, and what it re-exports.
                for s in self.exports(m, module) {
                    self.symbol_completion(s, out);
                }
                if let Some(names) = self.res.names.get(&m) {
                    for key in names.public.keys() {
                        out.push(key, CompletionKind::Module, None);
                    }
                }
                return;
            }
            Some((m, used)) => (self.res.lookup(m, segments[used]), &segments[used + 1..]),
            None => (self.res.lookup_in_file(module, file, head), &segments[1..]),
        };
        // Each remaining segment is a step into a C++ namespace. A head that resolves to
        // nothing still reaches the type queries below: `T::` on a generic parameter names
        // no symbol and answers them all the same (spec §5.5).
        let mut walked = sym;
        for seg in rest {
            walked = walked.and_then(|s| self.res.inner(s, seg));
        }
        let Some(sym) = walked else {
            for q in TYPE_QUERIES {
                out.push(q, CompletionKind::Constant, None);
            }
            return;
        };
        if self.res.symbol(sym).kind == SymbolKind::Namespace {
            // A namespace holds declarations, which keep their own kinds.
            let names: Vec<String> =
                self.res.symbol(sym).members.iter().map(|m| m.name.clone()).collect();
            for name in names {
                if let Some(inner) = self.res.inner(sym, &name) {
                    self.symbol_completion(inner, out);
                }
            }
            return;
        }
        {
            for m in &self.res.symbol(sym).members {
                let kind = match m.sort {
                    MemberSort::Variant => CompletionKind::Variant,
                    MemberSort::Method => CompletionKind::Method,
                    MemberSort::AssocType => CompletionKind::Type,
                    MemberSort::Const => CompletionKind::Constant,
                    MemberSort::Field | MemberSort::Ctor => continue,
                };
                out.push(&m.name, kind, None);
            }
        }
        // Every type answers these, a generic parameter included (spec §5.5).
        for q in TYPE_QUERIES {
            out.push(q, CompletionKind::Constant, None);
        }
    }
}

// ---------------------------------------------------------------------------
// completion
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CompletionKind {
    Keyword,
    Type,
    Struct,
    Enum,
    Trait,
    Function,
    Method,
    Field,
    Variant,
    Constant,
    Variable,
    Module,
    Macro,
}

#[derive(Clone, Debug)]
pub struct Completion {
    pub label: String,
    pub kind: CompletionKind,
    pub detail: Option<String>,
    /// What choosing it writes, when that is not the label: `rl::DrawLine` for a name of a
    /// dependency the manifest does not open (spec §3.2, §17.4).
    pub insert: Option<String>,
    /// The dependency key it comes from, shown beside it, when it is not in scope bare.
    pub source: Option<String>,
}

/// What [`completion`] offers, and whether the editor must ask again as more is typed —
/// true when names of dependencies that are not opened were left out for want of a prefix
/// to filter them by, since offering every one of a C library's would bury the rest.
#[derive(Debug, Default)]
pub struct CompletionSet {
    pub items: Vec<Completion>,
    pub incomplete: bool,
}

/// The keywords of spec §2.7. A test holds this to the lexer's own list.
pub const KEYWORDS: &[&str] = &[
    "as",
    "break",
    "case",
    "const",
    "continue",
    "defer",
    "else",
    "enum",
    "extern",
    "false",
    "fn",
    "for",
    "if",
    "ignore",
    "impl",
    "import",
    "in",
    "let",
    "macro",
    "module",
    "move",
    "mut",
    "none",
    "null",
    "or",
    "override",
    "own",
    "protected",
    "pub",
    "return",
    "self",
    "Self",
    "static",
    "struct",
    "switch",
    "trait",
    "true",
    "unreachable",
    "union",
    "unsafe",
    "virtual",
    "while",
];

/// One slot per label, first come first served.
#[derive(Default)]
struct Completions {
    items: Vec<Completion>,
    seen: HashSet<String>,
}

impl Completions {
    fn push(&mut self, label: &str, kind: CompletionKind, detail: Option<String>) {
        if self.seen.insert(label.to_owned()) {
            self.items.push(Completion {
                label: label.to_owned(),
                kind,
                detail,
                insert: None,
                source: None,
            });
        }
    }

    /// A name written through a dependency key. One slot per key and label, so two
    /// dependencies exporting `Vec3` are both offered; never over a name already in scope.
    fn push_qualified(
        &mut self,
        key: &str,
        label: &str,
        kind: CompletionKind,
        detail: Option<String>,
    ) {
        if self.seen.contains(label) || !self.seen.insert(format!("{key}::{label}")) {
            return;
        }
        self.items.push(Completion {
            label: label.to_owned(),
            kind,
            detail,
            insert: Some(format!("{key}::{label}")),
            source: Some(key.to_owned()),
        });
    }
}

/// Does `label` match what has been typed, the way an editor filters: the first character
/// the same, ignoring case, and the rest in order. `DrawLi` matches `DrawLine`, `dl` does
/// not match `DrawLine`.
fn fuzzy_match(typed: &str, label: &str) -> bool {
    let mut want = typed.chars().flat_map(char::to_lowercase);
    let mut have = label.chars().flat_map(char::to_lowercase);
    let (Some(first), Some(label_first)) = (want.next(), have.next()) else { return false };
    if first != label_first {
        return false;
    }
    'outer: for w in want {
        for h in have.by_ref() {
            if h == w {
                continue 'outer;
            }
        }
        return false;
    }
    true
}

fn is_ident_char(c: char) -> bool {
    c == '_' || c.is_alphanumeric()
}

fn trailing_ident(s: &str) -> &str {
    let len: usize = s.chars().rev().take_while(|c| is_ident_char(*c)).map(char::len_utf8).sum();
    &s[s.len() - len..]
}

/// The segments of a `::` path ending at `s`, outermost first.
///
/// `"let c = rl::MouseButton"` gives `["rl", "MouseButton"]`. Only what is joined by `::`
/// counts, so the code before the path is left behind at the first segment that is not.
fn path_segments(s: &str) -> Vec<&str> {
    let mut rest = s;
    let mut out = Vec::new();
    loop {
        let seg = trailing_ident(rest);
        if seg.is_empty() {
            break;
        }
        out.push(seg);
        rest = rest[..rest.len() - seg.len()].trim_end();
        match rest.strip_suffix("::") {
            Some(before) => rest = before.trim_end(),
            None => break,
        }
    }
    out.reverse();
    out
}

fn ident_prefix(s: &str) -> &str {
    let len: usize = s.chars().take_while(|c| is_ident_char(*c)).map(char::len_utf8).sum();
    &s[..len]
}

/// What could be typed at `offset` in `text`.
///
/// `current` is an analysis of exactly `text` when the server has one, which lets a `.`
/// complete on whatever expression precedes it. `fallback` is the latest analysis that
/// parsed cleanly: the half-typed line can break the code around it badly enough that
/// locals vanish, and a moment ago they were all there. Either may be absent or, in tests,
/// be of slightly different text; the context (`.`, `::`, a bare name) is always read from
/// `text` itself.
pub fn complete(
    current: Option<(&Analysis, FileId)>,
    fallback: Option<(&Analysis, FileId)>,
    text: &str,
    offset: usize,
) -> Vec<Completion> {
    completion(current, fallback, text, offset).items
}

/// [`complete`], saying also whether the editor must ask again as more is typed.
pub fn completion(
    current: Option<(&Analysis, FileId)>,
    fallback: Option<(&Analysis, FileId)>,
    text: &str,
    offset: usize,
) -> CompletionSet {
    let mut offset = offset.min(text.len());
    while !text.is_char_boundary(offset) {
        offset -= 1;
    }
    let before = &text[..offset];
    let line = before.rsplit(['\n', '\r']).next().unwrap_or("");
    // Nothing to offer inside a comment or a string.
    if line.contains("//") || line.matches('"').count() % 2 == 1 {
        return CompletionSet::default();
    }
    let lead = &before[..before.len() - trailing_ident(before).len()];
    let current = current.and_then(|(a, file)| Some((Query::new(a)?, file)));
    let fallback = fallback.and_then(|(a, file)| Some((Query::new(a)?, file)));
    let analyses: Vec<&(Query, FileId)> = current.iter().chain(fallback.iter()).collect();
    let mut out = Completions::default();

    if let Some(rest) = lead.strip_suffix("::") {
        let segments = path_segments(rest);
        for (q, file) in &analyses {
            q.complete_path(*file, &segments, &mut out);
        }
        return CompletionSet { items: out.items, incomplete: false };
    }
    if let Some(rest) = lead.strip_suffix('.') {
        let recv = trailing_ident(rest);
        // `0..` is a range and `1.` a float; neither has members.
        if rest.ends_with('.') || recv.starts_with(|c: char| c.is_ascii_digit()) {
            return CompletionSet { items: out.items, incomplete: false };
        }
        let dot = (lead.len() - 1) as u32;
        if let Some((q, file)) = &current {
            if q.a.map.get(*file).text() == text {
                if let Some((body, ty)) = q.receiver(*file, dot) {
                    q.members_of(body, &ty, &mut out);
                    return CompletionSet { items: out.items, incomplete: false };
                }
            }
        }
        if !recv.is_empty() {
            for (q, file) in &analyses {
                if q.members_by_name(*file, recv, offset as u32, &mut out) {
                    break;
                }
            }
        }
        return CompletionSet { items: out.items, incomplete: false };
    }

    for (q, file) in &analyses {
        q.complete_scope(*file, offset as u32, &mut out);
    }
    // Names of dependencies that are not opened, written with their key — only once
    // something is typed to narrow them (spec §17.4).
    let typed = before.len() - lead.len();
    let incomplete = typed == 0;
    if let Some((q, file)) = analyses.first() {
        if !incomplete {
            q.complete_through_keys(*file, &before[lead.len()..], &mut out);
        }
    }
    for k in KEYWORDS {
        out.push(k, CompletionKind::Keyword, None);
    }
    for t in BUILTIN_TYPES {
        out.push(t, CompletionKind::Type, None);
    }
    for f in BUILTIN_FNS {
        out.push(f, CompletionKind::Function, None);
    }
    CompletionSet { items: out.items, incomplete }
}

// ---------------------------------------------------------------------------
// inlay hints, signature help, workspace symbols
// ---------------------------------------------------------------------------

/// Text the editor shows inline that is not in the file.
#[derive(Debug)]
pub struct Hint {
    pub offset: u32,
    pub label: String,
    pub kind: HintKind,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HintKind {
    /// `: T` after a binding that does not write its type.
    Type,
    /// `name:` before a value in a positional struct literal, naming the field it fills.
    Field,
}

/// `: T` after every `let`, `for` and `case` binding in `range` that leaves its type to
/// inference (spec §17.4 item 7), and `name:` before every value of a positional struct
/// literal — `Movie{ 1, 7.1, "Heat" }` is legal only when every field is listed (§7.3), so
/// the reader otherwise has to count.
pub fn inlay_hints(a: &Analysis, file: FileId, range: Span) -> Vec<Hint> {
    let (Some(hir), Some(res)) = (a.hir.as_ref(), a.resolution.as_ref()) else { return Vec::new() };
    let text = a.map.get(file).text();
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for (_, body) in hir.bodies().filter(|(_, b)| b.file == file) {
        for l in &body.locals {
            if l.kind == LocalKind::Param
                || l.name == "_"
                || l.span.end < range.start
                || l.span.start > range.end
            {
                continue;
            }
            // Only where the binding is spelled out; the checker's own locals (`self`, whose
            // span is its method's name) have nowhere to hang a hint.
            if text.get(l.span.range()) != Some(l.name.as_str()) {
                continue;
            }
            // A written type needs no hint.
            let after = text.get(l.span.end as usize..).unwrap_or("");
            if after.trim_start_matches([' ', '\t']).starts_with(':') {
                continue;
            }
            let ty = type_name_in_file(res, file, &l.ty);
            if ty.contains("{unknown}") {
                continue;
            }
            if seen.insert(l.span.end) {
                out.push(Hint {
                    offset: l.span.end,
                    label: format!(": {ty}"),
                    kind: HintKind::Type,
                });
            }
        }
        if let Some(table) = a.table.as_ref() {
            field_hints(body, table, text, range, &mut out);
        }
    }
    out.sort_by_key(|h| h.offset);
    out
}

/// The field each positional value of a struct literal or union payload lands in.
fn field_hints(body: &Body, table: &TypeTable, text: &str, range: Span, out: &mut Vec<Hint>) {
    for e in &body.exprs {
        let (fields, values): (&[FieldSig], &[Option<ExprId>]) = match &e.kind {
            ExprKind::StructLit { ty, fields } => match table.struct_sig(*ty) {
                Some(s) => (&s.fields, fields),
                None => continue,
            },
            ExprKind::UnionVariant { owner, index, payload } => match table.get(*owner) {
                Some(ItemSig::Union { variants }) => match variants.get(*index) {
                    Some(v) => (&v.fields, payload),
                    None => continue,
                },
                _ => continue,
            },
            _ => continue,
        };
        for (field, value) in fields.iter().zip(values) {
            let Some(v) = value else { continue };
            let span = body.expr(*v).span;
            if span.start < range.start || span.start > range.end {
                continue;
            }
            // Positional values only: one that follows `{` or `,` has no name written, while
            // one after `name:` already says which field it is.
            let before = text.get(..span.start as usize).unwrap_or("").trim_end();
            if !before.ends_with(['{', ',']) {
                continue;
            }
            // `Vec3{ x, y, z }` names its fields already, by the variables it passes.
            if text.get(span.range()) == Some(field.name.as_str()) {
                continue;
            }
            out.push(Hint {
                offset: span.start,
                label: format!("{}:", field.name),
                kind: HintKind::Field,
            });
        }
    }
}

/// The signature of the call the cursor is inside.
pub struct Signature {
    /// The declaration's header, as hover shows it.
    pub label: String,
    /// Each parameter's byte range within `label`, the receiver left out.
    pub params: Vec<(usize, usize)>,
    /// Which argument the cursor is in.
    pub active: usize,
    pub docs: String,
}

pub fn signature_help(a: &Analysis, file: FileId, offset: u32) -> Option<Signature> {
    let q = Query::new(a)?;
    let text = a.map.get(file).text();
    let tokens = dz_lex::lex(file, text).tokens;

    // The innermost `(` still open at the cursor, and the commas before the cursor in it.
    let mut depth = 0usize;
    let mut commas = 0;
    let mut open = None;
    for (j, t) in tokens.iter().enumerate().rev() {
        if t.span.end > offset || t.kind == TokenKind::Eof {
            continue;
        }
        match t.kind {
            TokenKind::RParen | TokenKind::RBracket | TokenKind::RBrace => depth += 1,
            TokenKind::LParen if depth == 0 => {
                open = Some(j);
                break;
            }
            TokenKind::LParen | TokenKind::LBracket | TokenKind::LBrace => {
                // An unclosed `[` or `{` means the cursor is not in an argument list.
                if depth == 0 {
                    return None;
                }
                depth -= 1;
            }
            TokenKind::Comma if depth == 0 => commas += 1,
            _ => {}
        }
    }

    // The callee is the name before `(`, stepping back over `<...>` type arguments.
    let mut k = open?.checked_sub(1)?;
    if matches!(tokens[k].kind, TokenKind::Gt | TokenKind::Shr) {
        let mut angle = 0i32;
        loop {
            match tokens[k].kind {
                TokenKind::Gt => angle += 1,
                TokenKind::Shr => angle += 2,
                TokenKind::Lt => angle -= 1,
                _ => {}
            }
            if angle <= 0 {
                break;
            }
            k = k.checked_sub(1)?;
        }
        k = k.checked_sub(1)?;
    }
    if !is_name(tokens[k].kind) {
        return None;
    }
    let decl = match q.target_at(file, &tokens, text, k)? {
        Target::Decl(_, d) => d,
        Target::Ref(r) => {
            // An imported function has no Dazzle source to quote: its signature is the one
            // hover shows, rebuilt from the checker's types (spec §15.2).
            if let Ref::Symbol(s) = r {
                if let Some((head, docs, _, _)) = q.foreign_decl(s) {
                    return Some(Signature {
                        params: param_ranges(&head),
                        label: head,
                        active: commas,
                        docs: doxygen(&docs),
                    });
                }
            }
            let (f, s) = q.definition(&r)?;
            q.decl_at(f, s)?
        }
    };
    if !matches!(decl.kind, DeclKind::Function | DeclKind::Method | DeclKind::Ctor) {
        return None;
    }
    let docs = doxygen(&decl.docs);
    let params = param_ranges(&decl.header);
    // Every argument past the fixed ones belongs to a variadic last parameter (spec §7.2).
    let variadic = params.last().is_some_and(|&(s, e)| decl.header[s..e].contains(": .."));
    let active = if variadic { commas.min(params.len() - 1) } else { commas };
    Some(Signature { params, label: decl.header, active, docs })
}

/// The byte range of each parameter in a function header, without the receiver: callers
/// never pass `self` in the parentheses.
fn param_ranges(header: &str) -> Vec<(usize, usize)> {
    let Some(open) = header.find('(') else { return Vec::new() };
    let bytes = header.as_bytes();
    let mut depth = 0;
    let mut start = open + 1;
    let mut out = Vec::new();
    let push = |s: usize, e: usize, out: &mut Vec<(usize, usize)>| {
        let seg = &header[s..e];
        if !seg.trim().is_empty() {
            let lead = seg.len() - seg.trim_start().len();
            let trail = seg.len() - seg.trim_end().len();
            out.push((s + lead, e - trail));
        }
    };
    for (i, c) in header.char_indices().filter(|(i, _)| *i > open) {
        let arrow = c == '>' && i > 0 && bytes[i - 1] == b'-';
        match c {
            '(' | '[' | '<' => depth += 1,
            ')' | ']' | '>' if depth > 0 && !arrow => depth -= 1,
            ')' => {
                push(start, i, &mut out);
                break;
            }
            ',' if depth == 0 => {
                push(start, i, &mut out);
                start = i + 1;
            }
            _ => {}
        }
    }
    if let Some(&(s, e)) = out.first() {
        if matches!(&header[s..e], "self" | "&self" | "&mut self" | "mut self") {
            out.remove(0);
        }
    }
    out
}

pub struct SymbolMatch {
    pub name: String,
    pub kind: DeclKind,
    pub file: FileId,
    pub span: Span,
    /// The type or `impl` it is declared in.
    pub container: Option<String>,
}

/// Every declaration in the project whose name contains the query's letters in order,
/// ignoring case — the fuzzy match editors expect from "go to symbol".
pub fn workspace_symbols(a: &Analysis, query: &str) -> Vec<SymbolMatch> {
    let query: Vec<char> = query.to_lowercase().chars().collect();
    let matches = |name: &str| {
        let mut want = query.iter().peekable();
        for c in name.to_lowercase().chars() {
            if want.peek() == Some(&&c) {
                want.next();
            }
        }
        want.peek().is_none()
    };
    fn walk(
        decls: &[Decl],
        container: Option<&str>,
        file: FileId,
        out: &mut Vec<SymbolMatch>,
        matches: &dyn Fn(&str) -> bool,
    ) {
        for d in decls {
            if d.kind != DeclKind::Impl && matches(&d.name) {
                out.push(SymbolMatch {
                    name: d.name.clone(),
                    kind: d.kind,
                    file,
                    span: d.name_span,
                    container: container.map(str::to_owned),
                });
            }
            walk(&d.children, Some(&d.name), file, out, matches);
        }
    }
    let mut out = Vec::new();
    for f in a.files.values().flatten() {
        let source = a.map.get(f.file);
        if !is_pseudo(source.path()) {
            walk(&decls(source.text(), &f.ast), None, f.file, &mut out, &matches);
        }
    }
    out
}

// ---------------------------------------------------------------------------
// semantic tokens
// ---------------------------------------------------------------------------

/// What a name is, for colouring. The order is the legend the server announces.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TokenClass {
    Type,
    Struct,
    Enum,
    Interface,
    TypeParameter,
    Parameter,
    Variable,
    Property,
    EnumMember,
    Function,
    Method,
    Macro,
    /// A module named on the left of `::` — an `import`'s alias, and a C++ namespace an
    /// imported header brought in (spec §15.2). Without this a module reads as a variable,
    /// which is what it looks like to the grammar.
    Namespace,
}

impl TokenClass {
    pub const ALL: [TokenClass; 13] = [
        TokenClass::Type,
        TokenClass::Struct,
        TokenClass::Enum,
        TokenClass::Interface,
        TokenClass::TypeParameter,
        TokenClass::Parameter,
        TokenClass::Variable,
        TokenClass::Property,
        TokenClass::EnumMember,
        TokenClass::Function,
        TokenClass::Method,
        TokenClass::Macro,
        TokenClass::Namespace,
    ];
}

pub struct SemanticToken {
    pub span: Span,
    pub class: TokenClass,
    /// The name is being declared here, not used.
    pub declaration: bool,
    /// A constant.
    pub readonly: bool,
}

/// A class for every name in the file the compiler can resolve, in source order.
///
/// This is what the grammar cannot know — that `a` is a parameter and `b` a local, that
/// `Less` is a variant and `m_len` a field — and it comes from the same resolution hover
/// uses, so the colour of a name and what hover says about it cannot disagree. Names that
/// resolve to nothing (a builtin type, a generic parameter) are left to the grammar.
pub fn semantic_tokens(a: &Analysis, file: FileId) -> Vec<SemanticToken> {
    let Some(q) = Query::new(a) else { return Vec::new() };
    let text = a.map.get(file).text();
    let tokens = dz_lex::lex(file, text).tokens;
    let mut out = Vec::new();
    for (i, t) in tokens.iter().enumerate() {
        // `self` and `Self` are keywords, and the grammar colours them as such.
        if t.kind != TokenKind::Ident {
            continue;
        }
        let Some(target) = q.target_at(file, &tokens, text, i) else {
            // `rl::SetTargetFPS(60)` — the head of a qualified path may be a module rather
            // than a declaration in this file, and a module has no symbol of its own. It is
            // not a variable, which is what it would otherwise be coloured as.
            let before_path = tokens.get(i + 1).is_some_and(|n| n.kind == TokenKind::ColonColon);
            if before_path {
                let name = &text[t.span.start as usize..t.span.end as usize];
                // Or a public dependency further along one: the `units` of `phys::units::`.
                let mut start = i;
                while start >= 2
                    && tokens[start - 1].kind == TokenKind::ColonColon
                    && tokens[start - 2].kind == TokenKind::Ident
                {
                    start -= 2;
                }
                let path: Vec<&str> = (start..=i)
                    .step_by(2)
                    .map(|j| &text[tokens[j].span.start as usize..tokens[j].span.end as usize])
                    .collect();
                let nested = start < i
                    && q.res.module_prefix(file, &path).is_some_and(|(_, used)| used == path.len());
                if nested || q.res.module_alias(file, name).is_some() {
                    out.push(SemanticToken {
                        span: t.span,
                        class: TokenClass::Namespace,
                        declaration: false,
                        readonly: false,
                    });
                    continue;
                }
            }
            // After a `.`, a name with no declaration is still a member — of a type the
            // language provides, like `e.message` — and `.` is only ever member access.
            if i > 0 && tokens[i - 1].kind == TokenKind::Dot {
                let call = tokens.get(i + 1).is_some_and(|n| n.kind == TokenKind::LParen);
                let class = if call { TokenClass::Method } else { TokenClass::Property };
                out.push(SemanticToken {
                    span: t.span,
                    class,
                    declaration: false,
                    readonly: false,
                });
            }
            continue;
        };
        let (class, declaration, readonly) = match &target {
            Target::Decl(_, d) => {
                let (class, readonly) = decl_class(d.kind);
                (class, true, readonly)
            }
            Target::Ref(r) => match r {
                Ref::Local(b, l) => {
                    let local = b.local(*l);
                    let class = if local.kind == LocalKind::Param {
                        TokenClass::Parameter
                    } else {
                        TokenClass::Variable
                    };
                    (class, local.span == t.span, false)
                }
                Ref::Symbol(s) => {
                    let (class, readonly) = symbol_class(q.res.symbol(*s).kind);
                    (class, false, readonly)
                }
                Ref::Field(..) | Ref::VariantField(..) => (TokenClass::Property, false, false),
                Ref::Variant(..) => (TokenClass::EnumMember, false, false),
                // A constructor call is spelled with the type's name.
                Ref::Ctor(..) => (TokenClass::Struct, false, false),
                Ref::Method(..) => (TokenClass::Method, false, false),
            },
        };
        out.push(SemanticToken { span: t.span, class, declaration, readonly });
    }
    out
}

fn decl_class(kind: DeclKind) -> (TokenClass, bool) {
    match kind {
        DeclKind::Function => (TokenClass::Function, false),
        DeclKind::Method => (TokenClass::Method, false),
        DeclKind::Ctor | DeclKind::Struct => (TokenClass::Struct, false),
        DeclKind::Union | DeclKind::Enum => (TokenClass::Enum, false),
        DeclKind::Trait => (TokenClass::Interface, false),
        DeclKind::Impl | DeclKind::TypeAlias => (TokenClass::Type, false),
        DeclKind::Const => (TokenClass::Variable, true),
        DeclKind::Macro => (TokenClass::Macro, false),
        DeclKind::Field => (TokenClass::Property, false),
        DeclKind::Variant => (TokenClass::EnumMember, false),
    }
}

/// The fixes offered for the diagnostics overlapping `range` (spec §17.4).
///
/// This knows nothing about any particular fix. A phase that can say exactly how to resolve
/// a diagnostic attaches the edit to it ([`dz_diag::Fix`]), and it appears here — so a new
/// quick fix is written once, where the compiler already worked the answer out, and needs
/// no change to the server at all.
///
/// The range is what the editor has selected, which for a bare cursor is empty; a fix is
/// offered when its diagnostic's primary span *contains or touches* that, so putting the
/// caret anywhere in a `switch` offers to fill its arms.
pub fn code_actions(a: &Analysis, file: FileId, range: Span) -> Vec<dz_diag::Fix> {
    a.diags
        .iter()
        .filter_map(|d| {
            let (f, span) = d.primary_span()?;
            let touches = f == file && span.start <= range.end && range.start <= span.end;
            touches.then(|| d.fix.clone()).flatten()
        })
        .collect()
}

fn symbol_class(kind: SymbolKind) -> (TokenClass, bool) {
    match kind {
        SymbolKind::Struct => (TokenClass::Struct, false),
        SymbolKind::Union | SymbolKind::Enum => (TokenClass::Enum, false),
        SymbolKind::Trait => (TokenClass::Interface, false),
        SymbolKind::Fn => (TokenClass::Function, false),
        SymbolKind::Const => (TokenClass::Variable, true),
        SymbolKind::Static => (TokenClass::Variable, false),
        SymbolKind::Macro => (TokenClass::Macro, false),
        SymbolKind::Namespace => (TokenClass::Namespace, false),
    }
}
