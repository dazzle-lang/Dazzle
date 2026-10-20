//! Editor queries inside a macro invocation: `tests/run/machine`, M10's criterion (spec §19).
//!
//! What a state's body holds is the user's own code, carried into the expansion with its
//! spans intact, so the editor answers for it exactly as it would for code written outside
//! a macro: the machine's parameters and the state's fields are in scope, and a `.` offers
//! the members of what is before it (spec §12.4, §17.4).

use std::path::{Path, PathBuf};

use dz_driver::{analyse, analyse_with, Analysis, Mode};
use dz_lsp::ide;
use dz_span::FileId;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/run/machine")
}

fn main_file(a: &Analysis) -> FileId {
    a.map.iter().find(|f| f.path().ends_with("main.dz")).expect("main.dz").id()
}

/// The fixture with `typed` inserted on a new line above the line holding `needle`,
/// analysed as the server does, and the offset just after what was typed.
fn edited(needle: &str, typed: &str) -> (Analysis, FileId, String, usize) {
    let original = std::fs::read_to_string(root().join("src").join("main.dz")).unwrap();
    let i = original.find(needle).expect("needle");
    let line = original[..i].rfind('\n').map_or(0, |n| n + 1);
    let indent: String = original[line..].chars().take_while(|c| *c == ' ').collect();
    let text = format!("{}{indent}{typed}\n{}", &original[..line], &original[line..]);
    let offset = line + indent.len() + typed.len();
    let main = Path::new("src").join("main.dz");
    let overlay = |p: &Path| p.ends_with(&main).then(|| text.clone());
    let a = analyse_with(&root(), &overlay, Mode::Editor);
    let file = main_file(&a);
    (a, file, text, offset)
}

fn complete(needle: &str, typed: &str) -> Vec<String> {
    let (a, f, text, offset) = edited(needle, typed);
    ide::complete(Some((&a, f)), None, &text, offset).into_iter().map(|c| c.label).collect()
}

#[test]
fn the_fixture_checks_cleanly() {
    let a = analyse(&root());
    assert!(!a.diags.has_errors(), "{:?}", a.diags.iter().map(|d| &d.message).collect::<Vec<_>>());
    assert_eq!(a.expansions.len(), 1, "one `#machine`");
}

#[test]
fn a_dot_inside_a_state_offers_the_members_of_a_machine_parameter() {
    let got = complete("t += ctx.delta_time", "btn.");
    for want in ["scale", "sprite", "presses"] {
        assert!(got.iter().any(|l| l == want), "{want} missing from {got:?}");
    }
    assert!(!got.iter().any(|l| l == "let"), "no keywords after a dot");
}

#[test]
fn a_state_body_sees_the_parameters_and_its_own_fields() {
    let got = complete("t += ctx.delta_time", "");
    for want in ["ctx", "input", "btn", "t", "lerp"] {
        assert!(got.iter().any(|l| l == want), "{want} missing from {got:?}");
    }
    // The machine's own locals are hygienic, and never offered (spec §12.4).
    assert!(!got.iter().any(|l| l.contains('·')), "a hygienic name was offered: {got:?}");
}

#[test]
fn hover_inside_a_state_body_shows_the_parameter_type() {
    let a = analyse(&root());
    let f = main_file(&a);
    let text = a.map.get(f).text();
    let at = text.find("btn.sprite = Sprite::Selected").unwrap() as u32;
    let info = ide::info_at(&a, f, at).expect("something at `btn`");
    let hover = info.hover.unwrap_or_default();
    assert!(hover.contains("Button"), "{hover}");
}

#[test]
fn hover_on_the_invocation_names_the_macro() {
    let a = analyse(&root());
    let f = main_file(&a);
    let text = a.map.get(f).text();
    let at = text.find("#machine ButtonState").unwrap() as u32 + 2;
    let info = ide::info_at(&a, f, at).expect("something at `#machine`");
    assert!(info.hover.unwrap_or_default().contains("macro machine"));
    let (file, span) = info.definition.expect("its declaration");
    assert_eq!(&a.map.get(file).text()[span.range()], "machine");
}

#[test]
fn a_method_the_macro_declared_completes_on_the_union() {
    let got = complete("println(\"{} {}\", name(&state), btn.scale)", "state.");
    assert!(got.iter().any(|l| l == "step"), "{got:?}");
}
