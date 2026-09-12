//! Editor mode runs resolution and checking over whatever parsed, however broken — so it
//! must survive any half-typed program (the LSP runs it on every pause in typing, spec
//! §17.2), and it must still hide a later phase's errors while an earlier phase has failed.
//!
//! Every program in `tests/run` is damaged at a spread of token positions: a token deleted,
//! the file cut off, and a stray `.`, `(` or `{` inserted — the shapes a file is in while
//! someone types into it.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};

use dz_driver::{analyse_with, Analysis, Mode};
use dz_span::FileId;

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

/// What is wrong with an editor-mode analysis, if anything.
fn problems(a: &Analysis) -> Option<String> {
    if a.resolution.is_none() || a.hir.is_none() || a.table.is_none() {
        return Some("editor mode left the tree unchecked".to_owned());
    }
    let errors: Vec<u8> = a
        .diags
        .iter()
        .filter(|d| d.severity == dz_diag::Severity::Error)
        .map(|d| phase(d.code))
        .collect();
    // The first failing phase, counting lexing and parsing as one.
    let first = errors.iter().map(|&p| if p <= 2 { 2 } else { p }).min()?;
    let shown: Vec<u8> = a.diags.iter().map(|d| phase(d.code)).collect();
    let later = |p: u8| match first {
        2 => p > 2,
        3 => p > 3,
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
    let mut failures = Vec::new();
    for (root, text) in programs() {
        let name = root.file_name().unwrap().to_string_lossy().into_owned();
        for (what, broken) in damaged(&text) {
            match catch_unwind(AssertUnwindSafe(|| analyse(&root, &broken, Mode::Editor))) {
                Err(_) => failures.push(format!("{name}, {what}: the compiler panicked")),
                Ok(a) => {
                    if let Some(p) = problems(&a) {
                        failures.push(format!("{name}, {what}: {p}"));
                    }
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} failures, first ones:\n{}",
        failures.len(),
        failures.iter().take(15).cloned().collect::<Vec<_>>().join("\n")
    );
}

#[test]
fn editor_mode_reports_exactly_what_build_mode_reports() {
    for (root, text) in programs() {
        for broken in
            [text.clone(), text.replacen('{', "", 1), text.replacen("fn main", "fn 1main", 1)]
        {
            let build = analyse(&root, &broken, Mode::Build);
            let editor = analyse(&root, &broken, Mode::Editor);
            let codes = |a: &Analysis| {
                a.diags.iter().map(|d| (d.code, d.message.clone())).collect::<Vec<_>>()
            };
            assert_eq!(codes(&build), codes(&editor), "{}", root.display());
        }
    }
}

#[test]
fn build_mode_still_stops_at_a_syntax_error() {
    let (root, text) = programs().remove(0);
    let a = analyse(&root, &text.replacen('{', "", 1), Mode::Build);
    assert!(!a.parsed_cleanly);
    assert!(a.resolution.is_none() && a.hir.is_none());
}
