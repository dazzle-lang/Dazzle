//! Declarative macros (spec §12): matching an invocation against a macro's pattern, writing
//! its expansion, and replacing the invocation with the parsed result.
//!
//! Expansion runs on syntax alone, after parsing and `#if` and before name resolution
//! (spec §12.11, 18.74). It is a pure function of the parsed workspace: the macros a module
//! can see are its own and its dependencies' `pub` ones, all known before any expansion
//! runs, since an expansion may invoke a macro but never declare one (spec §12.5).

use std::collections::{HashMap, HashSet};

use dz_ast::*;
use dz_diag::{codes, Diagnostic, Diagnostics};
use dz_lex::{Token, TokenKind};
use dz_project::{ModuleId, Workspace};
use dz_resolve::ParsedFile;
use dz_span::{FileId, Span};

mod hygiene;
mod matcher;
mod table;
mod template;
mod walk;

pub use hygiene::is_hygienic;
pub use table::MacroKind;

use table::{MacroEntry, Table};

/// How deep expansions may invoke macros (spec §12.5).
const MAX_DEPTH: u32 = 64;

/// One invocation that was expanded, for what comes after: the "in expansion of" note on a
/// diagnostic in code the macro wrote, and the editor's hover over `#name` or `name!`.
#[derive(Clone, Debug)]
pub struct Record {
    pub file: FileId,
    /// The whole invocation.
    pub call: Span,
    /// `#name` or `name!`, which every token the macro wrote points at.
    pub name_span: Span,
    pub name: String,
    pub kind: MacroKind,
    /// Where the macro is declared: its file and its name.
    pub def_file: FileId,
    pub def_span: Span,
}

/// Expand every invocation in the workspace, in place, and say what was expanded.
pub fn expand(
    ws: &Workspace,
    files: &mut HashMap<ModuleId, Vec<ParsedFile>>,
    diags: &mut Diagnostics,
) -> Vec<Record> {
    let table = build_table(ws, files, diags);
    let mut records = Vec::new();
    let mut serial = 0;
    let mut ids: Vec<ModuleId> = files.keys().copied().collect();
    ids.sort();
    for module in ids {
        let Some(parsed) = files.get_mut(&module) else { continue };
        for pf in parsed.iter_mut() {
            let mut x = Expander {
                table: &table,
                module,
                file: pf.file,
                depth: 0,
                serial: &mut serial,
                records: &mut records,
                diags,
            };
            let items = std::mem::take(&mut pf.ast.items);
            pf.ast.items = x.items(items);
        }
    }
    records
}

/// Add "in expansion of `#name`" (or `name!`) to every diagnostic that points at code a
/// macro wrote — that is, at an invocation's name (spec §12.6) — and report a mistake once
/// however many
/// places an expansion copied it to.
pub fn annotate(records: &[Record], map: &dz_span::SourceMap, diags: &mut Diagnostics) {
    diags.dedup();
    if records.is_empty() {
        return;
    }
    let by_site: HashMap<(FileId, Span), &Record> =
        records.iter().map(|r| ((r.file, r.name_span), r)).collect();
    for d in diags.iter_mut() {
        let Some(site) = d.primary_span() else { continue };
        let Some(r) = by_site.get(&site) else { continue };
        let note =
            format!("in expansion of `{}`", macro_spelling(r.kind != MacroKind::Decl, &r.name));
        if d.notes.iter().any(|n| n.contains(&note)) {
            continue;
        }
        let at = map
            .try_get(r.def_file)
            .map(|f| {
                let loc = f.location(r.def_span.start as usize);
                format!(", declared at {}:{}:{}", f.name(), loc.line, loc.column)
            })
            .unwrap_or_default();
        d.notes.push(format!("note: {note}{at}"));
    }
}

fn build_table(
    ws: &Workspace,
    files: &HashMap<ModuleId, Vec<ParsedFile>>,
    diags: &mut Diagnostics,
) -> Table {
    let mut table = Table::default();
    for m in ws.modules() {
        let deps =
            m.deps.iter().filter(|d| !d.implicit).map(|d| (d.alias.clone(), d.module)).collect();
        table.deps.insert(m.id, deps);
    }
    let mut ids: Vec<ModuleId> = files.keys().copied().collect();
    ids.sort();
    for module in ids {
        let mut names = HashSet::new();
        for pf in &files[&module] {
            for item in &pf.ast.items {
                if let Some(n) = top_name(item) {
                    names.insert(n.to_owned());
                }
                let ItemKind::Macro(decl) = &item.kind else { continue };
                let mut problems = table::validate(decl, pf.file);
                let kind = match table::macro_kind(decl, pf.file) {
                    Ok(k) => Some(k),
                    Err(d) => {
                        problems.push(*d);
                        None
                    }
                };
                let rejected = problems.iter().any(|d| d.severity == dz_diag::Severity::Error);
                diags.extend(problems);
                let key = (module, decl.name.name.clone());
                if let Some(prev) = table.macros.get(&key) {
                    diags.push(
                        Diagnostic::error(
                            codes::DUPLICATE_MACRO,
                            format!("macro `{}` is declared twice in this module", decl.name.name),
                        )
                        .at(pf.file, decl.name.span, "a second declaration")
                        .with_secondary(
                            prev.file,
                            prev.decl.name.span,
                            "the first",
                        ),
                    );
                    continue;
                }
                table.macros.insert(
                    key,
                    MacroEntry {
                        decl: decl.clone(),
                        module,
                        file: pf.file,
                        vis: item.vis,
                        kind: if rejected { None } else { kind },
                    },
                );
            }
        }
        table.top_names.insert(module, names);
    }
    table
}

fn top_name(item: &Item) -> Option<&str> {
    Some(match &item.kind {
        ItemKind::Fn(f) => &f.name.name,
        ItemKind::Struct(s) => &s.name.name,
        ItemKind::Union(u) => &u.name.name,
        ItemKind::Enum(e) => &e.name.name,
        ItemKind::Trait(t) => &t.name.name,
        ItemKind::Const(c) => &c.name.name,
        ItemKind::Static(s) => &s.name.name,
        _ => return None,
    })
}

/// Where an invocation is, which decides what its macro may be (spec §12.5).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Position {
    Decl,
    Stmt,
    Expr,
}

impl Position {
    fn describe(self) -> &'static str {
        match self {
            Position::Decl => "where a declaration goes",
            Position::Stmt => "where a statement goes",
            Position::Expr => "where an expression goes",
        }
    }
}

/// What an expansion parsed to.
enum Out {
    Items(Vec<Item>),
    Stmts(Vec<Stmt>),
    Expr(Expr),
}

struct Expander<'a> {
    table: &'a Table,
    module: ModuleId,
    file: FileId,
    depth: u32,
    serial: &'a mut u32,
    records: &'a mut Vec<Record>,
    diags: &'a mut Diagnostics,
}

impl<'a> Expander<'a> {
    /// A list of declarations, each invocation among them replaced by what it expands to.
    fn items(&mut self, items: Vec<Item>) -> Vec<Item> {
        let mut out = Vec::with_capacity(items.len());
        for mut item in items {
            if let ItemKind::MacroInvocation(call) = &item.kind {
                if let Some(a) = item.attrs.first() {
                    self.diags.push(
                        Diagnostic::error(
                            codes::MACRO_WRONG_POSITION,
                            "an attribute cannot apply to a macro invocation",
                        )
                        .at(self.file, a.span, "on an invocation")
                        .with_help("write the attribute inside what the macro expands to"),
                    );
                }
                if let Some(Out::Items(mut new)) = self.expand(call, Position::Decl) {
                    if let Some(first) = new.first_mut() {
                        if first.docs.is_empty() {
                            first.docs = std::mem::take(&mut item.docs);
                        }
                    }
                    out.extend(new);
                }
                continue;
            }
            walk::item(self, &mut item);
            out.push(item);
        }
        out
    }

    /// Expand one invocation, and every invocation its expansion contains.
    fn expand(&mut self, call: &MacroCall, pos: Position) -> Option<Out> {
        if self.depth >= MAX_DEPTH {
            self.diags.push(
                Diagnostic::error(
                    codes::MACRO_TOO_DEEP,
                    format!("macros expand more than {MAX_DEPTH} deep here"),
                )
                .at(self.file, call.name_span, "the innermost of them")
                .with_help("a macro that invokes itself needs a case that does not (spec §12.5)"),
            );
            return None;
        }
        let table = self.table;
        let (entry, def_alias) = self.lookup(table, call, pos)?;
        let kind = entry.kind?;
        // The kind decides the spelling: `max!(…)` and `log!(…)` read like the call they stand
        // for, `#machine …` like the declaration it writes (spec §12.5).
        let spelled = macro_spelling(kind != MacroKind::Decl, &call.path_text());
        if call.bang != (kind != MacroKind::Decl) {
            let was = macro_spelling(call.bang, &call.path_text());
            self.diags.push(
                Diagnostic::error(
                    codes::MACRO_SPELLING,
                    format!("`{}` is a `{}` macro, invoked `{spelled}`", call.name(), kind.name()),
                )
                .at(self.file, call.name_span, format!("written `{was}`"))
                .with_secondary(entry.file, entry.decl.kind.span, "declared as this kind")
                .with_help(
                    "an `expr` or `stmt` macro is invoked `name!(…)`, a `decl` macro `#name …` \
                     (spec §12.5)",
                ),
            );
            return None;
        }
        let fits = match pos {
            Position::Decl => kind == MacroKind::Decl,
            Position::Stmt => true,
            Position::Expr => kind == MacroKind::Expr,
        };
        if !fits {
            self.diags.push(
                Diagnostic::error(
                    codes::MACRO_WRONG_POSITION,
                    format!("`{spelled}` is a `{}` macro, invoked {}", kind.name(), pos.describe()),
                )
                .at(self.file, call.name_span, "cannot be invoked here")
                .with_secondary(entry.file, entry.decl.kind.span, "declared as this kind")
                .with_help(
                    "a `decl` macro goes where a declaration may, a `stmt` macro where a \
                     statement may, and an `expr` macro where an expression may (spec §12.5)",
                ),
            );
            return None;
        }

        // Match.
        let mut toks: Vec<Token> = call.tokens.iter().map(|t| Token::new(t.kind, t.span)).collect();
        let mut texts: Vec<String> = call.tokens.iter().map(|t| t.text.clone()).collect();
        let end = Span::at(call.span.end as usize);
        toks.push(Token::new(TokenKind::Eof, end));
        texts.push(String::new());
        let pattern = entry.decl.pattern.as_deref()?;
        let mut m = matcher::Matcher::new(self.file, &entry.decl, &toks, &texts);
        let captures = m.run(pattern);
        let match_diags = std::mem::take(&mut m.diags);
        let Ok(captures) = captures else {
            for d in match_diags {
                self.diags.push(d.with_secondary(
                    entry.file,
                    entry.decl.name.span,
                    format!("`{spelled}` is declared here"),
                ));
            }
            return None;
        };

        // Write.
        let mut w = template::Writer {
            file: self.file,
            input: &call.tokens,
            name_span: call.name_span,
            out: Vec::new(),
            error: None,
        };
        w.write(entry.decl.expand.as_deref().unwrap_or(&[]), &captures, &mut Vec::new());
        if let Some(e) = w.error.take() {
            self.diags.push(e);
            return None;
        }
        let written = template::normalise(w.out);

        // Parse, as what the macro's kind says it is.
        let mut toks: Vec<Token> = written.iter().map(|t| Token::new(t.kind, t.span)).collect();
        let mut texts: Vec<String> = written.iter().map(|t| t.text.clone()).collect();
        toks.push(Token::new(TokenKind::Eof, call.name_span));
        texts.push(String::new());
        let (mut out, parse_diags) = match kind {
            MacroKind::Decl => {
                let (items, d) = dz_parse::parse_items(self.file, &toks, &texts);
                (Out::Items(items), d)
            }
            MacroKind::Stmt => {
                let (stmts, d) = dz_parse::parse_stmts(self.file, &toks, &texts);
                (Out::Stmts(stmts), d)
            }
            MacroKind::Expr => {
                let (mut e, d) = dz_parse::parse_expr(self.file, &toks, &texts);
                // The value is the invocation's, so that is where its diagnostics point.
                e.span = call.span;
                (Out::Expr(e), d)
            }
        };
        self.diags.extend(parse_diags);
        if let Out::Items(items) = &out {
            if let Some(i) = items.iter().find(|i| matches!(i.kind, ItemKind::Macro(_))) {
                self.diags.push(
                    Diagnostic::error(
                        codes::MACRO_IN_EXPANSION,
                        format!("`{spelled}` expands to a macro declaration"),
                    )
                    .at(self.file, i.span, "declares a macro")
                    .with_help(
                        "an expansion may invoke macros but not declare them, so every macro \
                         is known before any expansion runs (spec §12.5)",
                    ),
                );
                return None;
            }
        }

        // Hygiene, then whatever the expansion itself invokes.
        *self.serial += 1;
        let names = table.top_names.get(&entry.module);
        let def =
            def_alias.as_deref().zip(names).map(|(alias, names)| hygiene::DefSite { alias, names });
        let serial = *self.serial;
        match &mut out {
            Out::Items(items) => {
                hygiene::apply(hygiene::Output::Items(items), call.name_span, serial, def)
            }
            Out::Stmts(stmts) => {
                hygiene::apply(hygiene::Output::Stmts(stmts), call.name_span, serial, def)
            }
            Out::Expr(e) => hygiene::apply(hygiene::Output::Expr(e), call.name_span, serial, def),
        }
        self.records.push(Record {
            file: self.file,
            call: call.span,
            name_span: call.name_span,
            name: call.name().to_owned(),
            kind,
            def_file: entry.file,
            def_span: entry.decl.name.span,
        });

        self.depth += 1;
        let out = match out {
            Out::Items(items) => Out::Items(self.items(items)),
            Out::Stmts(mut stmts) => {
                walk::stmts(self, &mut stmts);
                Out::Stmts(stmts)
            }
            Out::Expr(mut e) => {
                walk::expr(self, &mut e);
                Out::Expr(e)
            }
        };
        self.depth -= 1;
        Some(out)
    }

    /// The macro `#name`, `name!`, or either through a `dep::`, names from here, and the alias it was reached
    /// through when it is a dependency's.
    fn lookup(
        &mut self,
        table: &'a Table,
        call: &MacroCall,
        pos: Position,
    ) -> Option<(&'a MacroEntry, Option<String>)> {
        let names: Vec<&str> = call.path.segments.iter().map(|s| s.name.name.as_str()).collect();
        let (target, name, alias) =
            match names[..] {
                [name] => (self.module, name, None),
                [alias, name] => match table.deps.get(&self.module).and_then(|d| d.get(alias)) {
                    Some(m) => (*m, name, Some(alias.to_owned())),
                    None => {
                        self.diags.push(
                        Diagnostic::error(
                            codes::UNKNOWN_MACRO,
                            format!("`{alias}` is not a dependency of this module"),
                        )
                        .at(self.file, call.path.segments[0].span, "not a dependency")
                        .with_help(
                            "a dependency's macro is `#dep::name` or `dep::name!`, with `dep` as \
                             `[dependencies]` names it (spec §12.5)",
                        ),
                    );
                        return None;
                    }
                },
                _ => {
                    self.diags.push(
                        Diagnostic::error(codes::UNKNOWN_MACRO, "a macro is `name` or `dep::name`")
                            .at(self.file, call.path.span, "too many segments"),
                    );
                    return None;
                }
            };
        let Some(entry) = table.macros.get(&(target, name.to_owned())) else {
            let mut d = Diagnostic::error(
                codes::UNKNOWN_MACRO,
                format!("there is no macro `{name}`"),
            )
            .at(self.file, call.name_span, "no macro by this name");
            if INTRINSICS.contains(&name) && pos != Position::Expr {
                let written = macro_spelling(FORMAT_MACROS.contains(&name), name);
                d = d.with_help(format!(
                    "`{written}` is an intrinsic, which is an expression (spec §12.7)"
                ));
            } else {
                let known =
                    table.macros.keys().filter(|(m, _)| *m == target).map(|(_, n)| n.as_str());
                if let Some(near) = dz_resolve::closest(name, known) {
                    let bang = table.macros.get(&(target, near.to_owned())).and_then(|e| e.kind)
                        != Some(MacroKind::Decl);
                    d = d.with_help(format!("did you mean `{}`?", macro_spelling(bang, &near)));
                }
            }
            self.diags.push(d);
            return None;
        };
        if target != self.module && entry.vis != Vis::Public {
            self.diags.push(
                Diagnostic::error(codes::UNKNOWN_MACRO, format!("macro `{name}` is private"))
                    .at(self.file, call.name_span, "not exported")
                    .with_secondary(entry.file, entry.decl.name.span, "declared without `pub`")
                    .with_help(
                        "only a `pub macro` can be invoked from another module (spec §12.5)",
                    ),
            );
            return None;
        }
        Some((entry, alias))
    }
}

impl walk::Hooks for Expander<'_> {
    fn stmts(&mut self, stmts: &mut Vec<Stmt>) {
        if !stmts.iter().any(|s| matches!(s.kind, StmtKind::Macro(_))) {
            return;
        }
        let old = std::mem::take(stmts);
        for s in old {
            let StmtKind::Macro(call) = &s.kind else {
                stmts.push(s);
                continue;
            };
            match self.expand(call, Position::Stmt) {
                Some(Out::Stmts(new)) => stmts.extend(new),
                Some(Out::Items(items)) => stmts.extend(items.into_iter().map(|i| Stmt {
                    span: i.span,
                    docs: Vec::new(),
                    kind: StmtKind::Item(Box::new(i)),
                })),
                Some(Out::Expr(e)) => {
                    stmts.push(Stmt { span: s.span, docs: s.docs, kind: StmtKind::Expr(e) })
                }
                None => {}
            }
        }
    }

    fn expr(&mut self, e: &mut Expr) {
        let ExprKind::MacroCall(call) = &e.kind else { return };
        let call = call.clone();
        *e = match self.expand(&call, Position::Expr) {
            Some(Out::Expr(x)) => x,
            _ => Expr { span: call.span, kind: ExprKind::Error },
        };
    }
}
