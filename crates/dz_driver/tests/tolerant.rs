//! Editor mode runs resolution and checking over whatever parsed, however broken — so it
//! must survive any half-typed program (the LSP runs it on every pause in typing, spec
//! §17.2), and it must still hide the phases whose answers an earlier failure made
//! meaningless: everything after a parse error, and ownership after a resolution error.
//! Type checking is *not* one of them — see `problems`.
//!
//! Every program in `tests/run` is damaged at a spread of token positions: a token deleted,
//! the file cut off, and a stray `.`, `(` or `{` inserted — the shapes a file is in while
//! someone types into it.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};

use dz_driver::{analyse_with, Analysis, Mode};
use dz_span::FileId;

mod parallel;

/// How many positions per program to damage. Enough to hit every construct in them.
const POSITIONS: usize = 40;

fn programs() -> Vec<(PathBuf, String)> {
    let run = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/run");
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&run).expect("tests/run") {
        let dir = entry.expect("entry").path();
        let main = dir.join("src").join("main.dz");
        if main.is_file() {
            let text = std::fs::read_to_string(&main).expect("main.dz");
            out.push((dir, text));
        }
    }
    out.sort();
    assert!(out.len() >= 5, "expected the tests/run programs");
    out
}

fn analyse(root: &Path, text: &str, mode: Mode) -> Analysis {
    let main = Path::new("src").join("main.dz");
    let overlay = |p: &Path| p.ends_with(&main).then(|| text.to_owned());
    analyse_with(root, &overlay, mode)
}

fn phase(code: Option<&str>) -> u8 {
    code.and_then(|c| c.get(2..4)).and_then(|p| p.parse().ok()).unwrap_or(0)
}

/// Loading the workspace reports these before anything is lexed, about the machine rather
/// than the program: libclang missing, a header unreadable, a CMake library not yet built
/// (the editor never runs CMake). They follow no phase, and whether they appear depends on
/// what was built before, so counting them made this test pass only after a `dz build`.
fn environmental(code: Option<&str>) -> bool {
    use dz_diag::codes;
    [codes::CPP_HEADER_FAILED, codes::CPP_UNAVAILABLE, codes::CPP_NOT_BUILT]
        .iter()
        .any(|c| code == Some(*c))
}

/// What is wrong with an editor-mode analysis, if anything.
fn problems(a: &Analysis) -> Option<String> {
    if a.resolution.is_none() || a.hir.is_none() || a.table.is_none() {
        return Some("editor mode left the tree unchecked".to_owned());
    }
    let diags = || a.diags.iter().filter(|d| !environmental(d.code));
    let errors: Vec<u8> =
        diags().filter(|d| d.severity == dz_diag::Severity::Error).map(|d| phase(d.code)).collect();
    // The first failing phase, counting lexing and parsing as one.
    let first = errors.iter().map(|&p| if p <= 2 { 2 } else { p }).min()?;
    let shown: Vec<u8> = diags().map(|d| phase(d.code)).collect();
    // Which phases must stay quiet, given the first one that failed. The code ranges are
    // not in pipeline order — ownership is `DZ04xx` and type checking `DZ05xx`, but
    // ownership runs second — so these are named rather than compared as `p > first`.
    let later = |p: u8| match first {
        // A parse error leaves a tree that is a guess. Nothing after it means anything.
        2 => p > 2,
        // Resolution failed. **Type checking still reports**: a diagnostic about an
        // expression built from a name that did not resolve is suppressed where it arises,
        // by the checker's own `Ty::Error` guards, which leaves the ones that never
        // touched it — the whole point of not gating the phase wholesale. Ownership is
        // still silent: it needs types it can trust.
        3 => p == 4,
        _ => false,
    };
    if shown.iter().any(|&p| later(p)) {
        return Some(format!("phase {first} failed, yet later phases reported: {shown:?}"));
    }
    None
}

fn damaged(text: &str) -> Vec<(String, String)> {
    let tokens = dz_lex::lex(FileId(0), text).tokens;
    let stride = (tokens.len() / POSITIONS).max(1);
    let mut out = Vec::new();
    for (n, t) in tokens.iter().enumerate().step_by(stride) {
        let (s, e) = (t.span.start as usize, t.span.end as usize);
        out.push((format!("token {n} deleted"), format!("{}{}", &text[..s], &text[e..])));
        out.push((format!("cut at token {n}"), text[..s].to_owned()));
        for stray in [".", "(", "{"] {
            out.push((
                format!("`{stray}` after token {n}"),
                format!("{}{stray}{}", &text[..e], &text[e..]),
            ));
        }
    }
    out
}

#[test]
fn editor_mode_survives_every_damaged_program() {
    // Some seven thousand analyses, no two of which can see each other's work, so they are
    // run on every core (`parallel`). Left as one serial loop this test alone was three
    // quarters of `cargo test --workspace`.
    //
    // All the damaged text is built up front: the corpus is under 100 KB, so two hundred
    // variants of every program is a few tens of megabytes, and having the whole list in
    // hand is what lets a spare thread pick up the next case rather than wait for the
    // program it was given.
    let cases: Vec<(PathBuf, String, String, String)> = programs()
        .into_iter()
        .flat_map(|(root, text)| {
            let name = root.file_name().unwrap().to_string_lossy().into_owned();
            damaged(&text)
                .into_iter()
                .map(move |(what, broken)| (root.clone(), name.clone(), what, broken))
        })
        .collect();

    let mut failures =
        parallel::filter_map(&cases, |(root, name, what, broken)| {
            match catch_unwind(AssertUnwindSafe(|| analyse(root, broken, Mode::Editor))) {
                Err(_) => Some(format!("{name}, {what}: the compiler panicked")),
                Ok(a) => problems(&a).map(|p| format!("{name}, {what}: {p}")),
            }
        });
    // The threads finish in whatever order the work fell out; the same break should read
    // the same way twice.
    failures.sort();

    assert!(
        failures.is_empty(),
        "{} failures, first ones:\n{}",
        failures.len(),
        failures.iter().take(15).cloned().collect::<Vec<_>>().join("\n")
    );
}

#[test]
fn editor_mode_reports_exactly_what_build_mode_reports() {
    parallel::for_each(&programs(), |(root, text)| {
        for broken in
            [text.clone(), text.replacen('{', "", 1), text.replacen("fn main", "fn 1main", 1)]
        {
            let build = analyse(root, &broken, Mode::Build);
            let editor = analyse(root, &broken, Mode::Editor);
            let codes = |a: &Analysis| {
                a.diags.iter().map(|d| (d.code, d.message.clone())).collect::<Vec<_>>()
            };
            assert_eq!(codes(&build), codes(&editor), "{}", root.display());
        }
    });
}

#[test]
fn build_mode_still_stops_at_a_syntax_error() {
    let (root, text) = programs().remove(0);
    let a = analyse(&root, &text.replacen('{', "", 1), Mode::Build);
    assert!(!a.parsed_cleanly);
    assert!(a.resolution.is_none() && a.hir.is_none());
}
