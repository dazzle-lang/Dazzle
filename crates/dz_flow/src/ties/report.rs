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
use dz_resolve::Resolution;
use dz_span::Span;

use super::body::{body_signature, BodyAnalysis, KillKind};
use super::{canonical_place, export_steps, minimal, ties, Mismatch, Place, Step};

/// Report what [`super::analyse_body`] found in `body`.
pub fn report(
    body: &Body,
    table: &TypeTable,
    res: &Resolution,
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
    // (§6.7): the type must declare a tie for the field first, which is the author's
    // choice of name, so the fix is only offered.
    let mut hidden: Option<String> = None;
    for p in &needed {
        let path = tie_path(table, res, body, p);
        let s = dz_ast::dump::tie_path_str(&path);
        if let Err(e) = dz_check::ties::resolve(&scope, &path) {
            if e.code == codes::TIE_PATH_NOT_VISIBLE {
                hidden.get_or_insert(s.clone());
            }
        }
        if !paths.contains(&s) {
            paths.push(s);
        }
    }
    let clause = if paths.is_empty() { None } else { Some(format!("kills {}", paths.join(", "))) };
    let (title, edit) = match (&clause, sig.kills_span) {
        (Some(c), Some(at)) => (format!("write `{c}`"), Edit { file, span: at, text: c.clone() }),
        (Some(c), None) => {
            let at = body.block(body.root).span.start;
            let at = at as usize;
            (format!("add `{c}`"), Edit { file, span: Span::new(at, at), text: format!("{c} ") })
        }
        (None, Some(at)) => {
            ("remove the clause".to_owned(), Edit { file, span: at, text: String::new() })
        }
        (None, None) => return,
    };
    // A function a macro wrote has every token at the invocation (§12), so its body does
    // not follow its name: the clause belongs in the macro, and no edit here can write it.
    let written_by_macro = body.block(body.root).span.start <= body.span.end;
    let fix = |d: Diagnostic| match (&hidden, written_by_macro) {
        (None, false) => d.with_fix(title.clone(), vec![edit.clone()]),
        (Some(_), false) => d.with_suggestion(title.clone(), vec![edit.clone()]),
        (_, true) => d.with_note("the function is written by a macro: its clause belongs there"),
    };
    let wanted = match (&clause, &hidden) {
        (Some(c), None) => format!("the body needs `{c}`"),
        (Some(c), Some(h)) => format!(
            "the body needs `{c}`, but `{h}` goes through a field `{name}`'s callers cannot \
             see; have the type declare a tie for it (`tie @name = …`) and name the tie"
        ),
        (None, _) => "the body kills nothing a caller can see".to_owned(),
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
    let ident = |name: &str| Ident::new(Span::DUMMY, name);
    let mut out = Vec::new();
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
                out.push(TieStep::Field(ident(name)));
                ty = next;
            }
            Step::Elements => {
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
    TiePath { span: Span::DUMMY, root: ident(&body.local(place.local).name), steps: out }
}
