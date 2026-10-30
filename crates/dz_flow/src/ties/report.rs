//! The tie analysis's findings as diagnostics, in terms of the user's code (spec §6.7).
//!
//! A place is shown as a tie path a signature could write: the parameter, its public
//! fields, and the ties a type declares rather than the private fields behind them, so
//! `SlotMap`'s `m_slot_of`'s array reads as `@items`. A clause mismatch carries the clause
//! the body needs, as a fix `dz fix` applies and the editor offers.

use dz_ast::{Ident, TiePath, TieStep};
use dz_check::TypeTable;
use dz_diag::{codes, Diagnostic, Diagnostics, Edit};
use dz_hir::ty::Ty;
use dz_hir::Body;
use dz_resolve::{Resolution, SymbolId};
use dz_span::{SourceMap, Span};

use super::body::{body_signature, BodyAnalysis, KillKind};
use super::{canonical_place, export_steps, minimal, ties, Mismatch, Place, Step};

/// Report what [`super::analyse_body`] found in `body`. `map` is only for laying out the
/// tie a fix declares.
pub fn report(
    body: &Body,
    table: &TypeTable,
    res: &Resolution,
    map: &SourceMap,
    a: &BodyAnalysis,
    diags: &mut Diagnostics,
) {
    let file = body.file;
    let show = |p: &Place| render(table, res, body, p);
    for u in &a.invalid_uses {
        let tie = show(&u.tie);
        let killed = show(&u.killed.place);
        // A `return` of a view into something the function owns, which dies as it returns.
        if u.killed.span == u.span && u.killed.kind == KillKind::Drop {
            diags.push(
                Diagnostic::error(
                    codes::USE_AFTER_KILL,
                    format!(
                        "this returns a view into `{killed}`, which dies as the function returns"
                    ),
                )
                .at(file, u.span, format!("points into `{tie}`"))
                .with_help(format!(
                    "`{killed}` is this function's own, a local or a parameter taken by value: \
                     return something owned (a `String` rather than a `str`), or take it by \
                     reference so what is returned outlives the call (spec §6.7)"
                )),
            );
            continue;
        }
        let how = match u.killed.kind {
            KillKind::Call => "this call kills",
            KillKind::Assign => "this write kills",
            KillKind::Move => "this move kills",
            KillKind::Drop => "this drop kills",
        };
        // The loop itself, going on over what its body moved: no use is written there.
        if u.next_pass {
            diags.push(
                Diagnostic::error(
                    codes::USE_AFTER_KILL,
                    format!("the loop goes on over memory its body killed: `{tie}`"),
                )
                .at(file, u.span, format!("the next pass reads `{tie}`"))
                .with_secondary(file, u.killed.span, format!("{how} `{killed}`"))
                .with_help(
                    "a loop walks its container's memory, so moving that memory ends the \
                     loop: `break` right after the call, or collect what to do and do it \
                     once the loop is over (spec §6.7)",
                ),
            );
            continue;
        }
        diags.push(
            Diagnostic::error(
                codes::USE_AFTER_KILL,
                format!("used after the memory it points into was killed: `{tie}`"),
            )
            .at(file, u.span, format!("points into `{tie}`"))
            .with_secondary(file, u.killed.span, format!("{how} `{killed}`"))
            .with_help(
                "a call, write, move or drop that can move or free memory ends every \
                 reference into it: use this before that happens, or get it again after \
                 (spec §6.7)",
            ),
        );
    }
    for r in &a.invalid_returns {
        let (actual, expected) = (show(&r.actual), show(&r.expected));
        diags.push(
            Diagnostic::error(
                codes::RETURN_OUTSIDE_TIE,
                format!("this points into `{actual}`, and the signature says `in {expected}`"),
            )
            .at(file, r.span, format!("points into `{actual}`"))
            .with_help(
                "a returned reference must point where its signature's `in` says, since \
                 that is what a caller trusts it to survive (spec §6.7)",
            ),
        );
    }
    if a.mismatches.is_empty() {
        return;
    }
    let Some(signature) = body_signature(body, table, res) else { return };
    let sig = &signature.sig;
    let name = &signature.name;
    let scope = signature.scope(table, res);
    let root = |p: &Place| body.local(p.local).ty.clone();
    // The clause the body needs: what it kills that a caller can see, each spelt as the
    // ties that name it, and nothing that names no memory.
    let needed: Vec<Place> = minimal(
        &a.actual
            .iter()
            .filter(|p| !ties(&scope, p, &root(p)).is_empty())
            .map(|p| canonical_place(table, res, &root(p), p))
            .collect::<Vec<_>>(),
    );
    let mut paths: Vec<String> = Vec::new();
    // A path through a field some caller cannot see is not a clause the function may write
    // (§6.7): the type must declare a tie for the field first. `ties` is that tie for each
    // such path, and `tied` the clause naming them, when every one has a tie to declare.
    let mut hidden: Option<String> = None;
    let mut new_ties = NewTies::default();
    let mut tied: Option<Vec<String>> = Some(Vec::new());
    for p in &needed {
        let (path, held) = tie_path_held(table, res, body, p);
        let s = dz_ast::dump::tie_path_str(&path);
        let mut named = Some(s.clone());
        if let Err(e) = dz_check::ties::resolve(&scope, &path) {
            if e.code == codes::TIE_PATH_NOT_VISIBLE {
                hidden.get_or_insert(s.clone());
                named = new_ties.add(table, &scope, &path, &held);
            }
        }
        match (&mut tied, named) {
            (Some(t), Some(n)) if !t.contains(&n) => t.push(n),
            (_, Some(_)) => {}
            (_, None) => tied = None,
        }
        if !paths.contains(&s) {
            paths.push(s);
        }
    }
    let clause = if paths.is_empty() { None } else { Some(format!("kills {}", paths.join(", "))) };
    // Where the clause goes: over the one written, or before the body's `{`.
    let write = |c: &Option<String>| match (c, sig.kills_span) {
        (Some(c), Some(at)) => {
            Some((format!("write `{c}`"), Edit { file, span: at, text: c.clone() }))
        }
        (Some(c), None) => {
            let at = body.block(body.root).span.start;
            let at = at as usize;
            Some((
                format!("add `{c}`"),
                Edit { file, span: Span::new(at, at), text: format!("{c} ") },
            ))
        }
        (None, Some(at)) => {
            Some(("remove the clause".to_owned(), Edit { file, span: at, text: String::new() }))
        }
        (None, None) => None,
    };
    let Some((title, edit)) = write(&clause) else { return };
    // With a hidden field, the fix declares the tie and writes the clause that names it.
    // The tie's name is the author's choice, so it is only offered; without one to
    // declare, the clause as it stands is offered, and the help says what is missing.
    let declaring = match (&hidden, tied) {
        (Some(_), Some(t)) => new_ties.edits(table, res, map).and_then(|decls| {
            let c = Some(format!("kills {}", t.join(", ")));
            let (_, edit) = write(&c)?;
            let shown: Vec<String> =
                new_ties.decls.iter().map(|d| format!("`{}`", d.text())).collect();
            let title = format!("declare {} and write `{}`", shown.join(", "), c?);
            Some((title, [decls, vec![edit]].concat(), shown))
        }),
        _ => None,
    };
    // A function a macro wrote has every token at the invocation (§12), so its body does
    // not follow its name: the clause belongs in the macro, and no edit here can write it.
    let written_by_macro = body.block(body.root).span.start <= body.span.end;
    let fix = |d: Diagnostic| match (&hidden, &declaring, written_by_macro) {
        (_, _, true) => d.with_note("the function is written by a macro: its clause belongs there"),
        (None, _, false) => d.with_fix(title.clone(), vec![edit.clone()]),
        (Some(_), Some((title, edits, _)), false) => {
            d.with_suggestion(title.clone(), edits.clone())
        }
        (Some(_), None, false) => d.with_suggestion(title.clone(), vec![edit.clone()]),
    };
    let wanted = match (&clause, &hidden, &declaring) {
        (Some(c), None, _) => format!("the body needs `{c}`"),
        (Some(c), Some(h), Some((_, _, shown))) => format!(
            "the body needs `{c}`, but `{h}` goes through a field `{name}`'s callers cannot \
             see; have the type declare a tie for it and name the tie, as in {}",
            shown.join(", ")
        ),
        (Some(c), Some(h), None) => format!(
            "the body needs `{c}`, but `{h}` goes through a field `{name}`'s callers cannot \
             see; have the type declare a tie for it (`tie @name = …`) and name the tie"
        ),
        (None, _, _) => "the body kills nothing a caller can see".to_owned(),
    };
    for m in &a.mismatches {
        let d = match m {
            Mismatch::Missing { actual } => {
                let place = &a.actual[*actual];
                let at = a.kills.iter().find(|k| place.covers(&k.place)).map(|k| k.span);
                let shown = show(&canonical_place(table, res, &root(place), place));
                Diagnostic::error(
                    codes::KILL_NOT_DECLARED,
                    format!("`{name}` kills `{shown}`, and its `kills` clause does not say so"),
                )
                .at(file, at.unwrap_or(body.span), format!("kills `{shown}`"))
                .with_help(format!(
                    "a signature says what a call may kill, so a caller knows which of its \
                     references survive: {wanted} (spec §6.7)"
                ))
            }
            Mismatch::Unused { declared } => {
                let path = &sig.kills[*declared];
                let shown = dz_ast::dump::tie_path_str(path);
                Diagnostic::error(
                    codes::KILL_NEVER_HAPPENS,
                    format!("`{name}` never kills `{shown}`"),
                )
                .at(file, path.span, "nothing here dies in the body")
                .with_help(format!(
                    "a `kills` clause says exactly what the body kills, so a caller's \
                     references are not ended for nothing: {wanted} (spec §6.7)"
                ))
            }
            Mismatch::TooBroad { declared, .. } => {
                let path = &sig.kills[*declared];
                let shown = dz_ast::dump::tie_path_str(path);
                Diagnostic::error(
                    codes::KILL_TOO_BROAD,
                    format!("`{shown}` is wider than what `{name}` kills"),
                )
                .at(file, path.span, "more than the body kills")
                .with_help(format!(
                    "a `kills` clause names exactly what dies, so a caller's other \
                     references survive the call: {wanted} (spec §6.7)"
                ))
            }
        };
        diags.push(fix(d));
    }
}

/// `place` as a tie path, shown.
fn render(table: &TypeTable, res: &Resolution, body: &Body, place: &Place) -> String {
    dz_ast::dump::tie_path_str(&tie_path(table, res, body, place))
}

/// `place` as a tie path: its local, then each type's declared ties where the steps are
/// what a tie stands for, and fields, `[*]` otherwise.
fn tie_path(table: &TypeTable, res: &Resolution, body: &Body, place: &Place) -> TiePath {
    tie_path_held(table, res, body, place).0
}

/// [`tie_path`], with the type holding each of its steps: what `.field` is a field of.
fn tie_path_held(
    table: &TypeTable,
    res: &Resolution,
    body: &Body,
    place: &Place,
) -> (TiePath, Vec<Ty>) {
    let ident = |name: &str| Ident::new(Span::DUMMY, name);
    let mut out = Vec::new();
    let mut held = Vec::new();
    let mut ty = body.local(place.local).ty.peeled().clone();
    let steps = &place.steps;
    let mut i = 0;
    'outer: while i < steps.len() {
        if let Ty::Named { sym, args } = ty.peeled_refs() {
            for decl in dz_check::ties::tie_decls(table, *sym) {
                let Some(name) = &decl.name else { continue };
                let Some(first) = decl.paths.first() else { continue };
                let Some((rep, end)) = export_steps(table, res, *sym, args, first) else {
                    continue;
                };
                if !rep.is_empty() && steps[i..].starts_with(&rep) {
                    held.push(ty.clone());
                    out.push(TieStep::Tie(ident(name)));
                    i += rep.len();
                    ty = end;
                    continue 'outer;
                }
            }
        }
        match &steps[i] {
            Step::Field(name) => {
                // A field the type does not have — one reached through a cast, such as a
                // `^mut void` read as a struct — cannot be written: the path stops at what
                // holds it, which names everything under it (§6.7).
                let Some(next) = super::field_of(table, &ty, name) else { break };
                held.push(ty.clone());
                out.push(TieStep::Field(ident(name)));
                ty = next;
            }
            Step::Elements => {
                held.push(ty.clone());
                out.push(TieStep::Elements(Span::DUMMY));
                ty = match &ty {
                    Ty::Ptr { inner, .. } | Ty::OwnPtr { inner, .. } => (**inner).clone(),
                    _ => dz_check::ties::elements_in(table, &ty).flatten().unwrap_or(Ty::Error),
                };
            }
            // The memory behind a pointer no tie names: only private code reaches it.
            Step::Storage => {}
        }
        i += 1;
    }
    let root = ident(&body.local(place.local).name);
    (TiePath { span: Span::DUMMY, root, steps: out }, held)
}

/// The ties a fix declares so that a clause can name memory behind a field its function's
/// callers cannot see (§6.7): one per hidden field, on the type the field belongs to, named
/// after the field without its `m_`, as `tie @things = m_things@items` names `m_things`.
#[derive(Default)]
struct NewTies {
    decls: Vec<NewTie>,
}

struct NewTie {
    owner: SymbolId,
    field: String,
    name: String,
    /// What it stands for, from the owner's own fields: every path the body needs through
    /// the field, so one tie names them all (r83).
    paths: Vec<String>,
}

impl NewTie {
    fn text(&self) -> String {
        format!("tie @{} = {}", self.name, self.paths.join(", "))
    }
}

impl NewTies {
    /// Declare a tie for the first field of `path` the function's callers cannot see, and
    /// return `path` naming that tie instead. `None` when no tie can stand for it: a step
    /// that is not a field, a field of no struct, or a name the type already declares.
    fn add(
        &mut self,
        table: &TypeTable,
        scope: &dz_check::ties::Scope<'_>,
        path: &TiePath,
        held: &[Ty],
    ) -> Option<String> {
        // The hidden step ends the shortest prefix refused for being hidden.
        let k = (0..path.steps.len()).find(|&k| {
            let prefix = TiePath { steps: path.steps[..=k].to_vec(), ..path.clone() };
            matches!(dz_check::ties::resolve(scope, &prefix),
                Err(e) if e.code == codes::TIE_PATH_NOT_VISIBLE)
        })?;
        let TieStep::Field(field) = &path.steps[k] else { return None };
        let Ty::Named { sym, .. } = held.get(k)?.peeled_refs() else { return None };
        let below =
            TiePath { span: Span::DUMMY, root: field.clone(), steps: path.steps[k + 1..].to_vec() };
        let below = dz_ast::dump::tie_path_str(&below);
        let name = field.name.strip_prefix("m_").unwrap_or(&field.name).to_owned();
        match self.decls.iter_mut().find(|d| d.owner == *sym && d.field == field.name) {
            Some(d) => {
                if !d.paths.contains(&below) {
                    d.paths.push(below);
                }
            }
            None => {
                // A second tie of one name is `DZ0566`, so a name taken is no fix.
                let declared = dz_check::ties::tie_decls(table, *sym);
                let taken = declared.iter().any(|t| t.name.as_deref() == Some(name.as_str()))
                    || self.decls.iter().any(|d| d.owner == *sym && d.name == name);
                if taken {
                    return None;
                }
                let decl = NewTie {
                    owner: *sym,
                    field: field.name.clone(),
                    name: name.clone(),
                    paths: vec![below],
                };
                self.decls.push(decl);
            }
        }
        let mut steps = path.steps[..k].to_vec();
        steps.push(TieStep::Tie(Ident::new(Span::DUMMY, &name)));
        Some(dz_ast::dump::tie_path_str(&TiePath { steps, ..path.clone() }))
    }

    /// One insertion per type: its new ties, each on a line of its own after the ties it
    /// declares already, or after its own fields, with a blank line, when it has none. `None`
    /// rather than an edit that is only probably right: a field it inherited, whose tie
    /// belongs on the base, or a type whose last member does not end its line.
    fn edits(&self, table: &TypeTable, res: &Resolution, map: &SourceMap) -> Option<Vec<Edit>> {
        let mut owners: Vec<SymbolId> = Vec::new();
        for d in &self.decls {
            if !owners.contains(&d.owner) {
                owners.push(d.owner);
            }
        }
        let mut out = Vec::new();
        for owner in owners {
            let sig = table.struct_sig(owner)?;
            let own = &sig.fields[sig.inherited..];
            let decls: Vec<&NewTie> = self.decls.iter().filter(|d| d.owner == owner).collect();
            for d in &decls {
                own.iter().find(|f| f.name == d.field && f.declared_in.is_none())?;
            }
            let file = res.symbol(owner).file;
            let text = map.try_get(file)?.text();
            let (last, gap) = match sig.ties.iter().map(|t| t.span.end).max() {
                Some(end) => (end, false),
                None => (own.iter().map(|f| f.span.end).max()?, true),
            };
            let last = last as usize;
            let line_start = text.get(..last)?.rfind('\n').map_or(0, |n| n + 1);
            let line_end = text[last..].find('\n').map_or(text.len(), |n| last + n);
            // `struct S { m_x: List<i32> }` on one line has nowhere to put a member.
            let rest = &text[last..line_end];
            if rest.contains('}') {
                return None;
            }
            let indent: String =
                text[line_start..].chars().take_while(|c| *c == ' ' || *c == '\t').collect();
            let (at, nl) = match text[..line_end].strip_suffix('\r') {
                Some(before) => (before.len(), "\r\n"),
                None => (line_end, "\n"),
            };
            let mut insert = String::new();
            if gap {
                insert.push_str(nl);
            }
            for d in decls {
                insert.push_str(nl);
                insert.push_str(&indent);
                insert.push_str(&d.text());
            }
            out.push(Edit { file, span: Span::new(at, at), text: insert });
        }
        Some(out)
    }
}
