//! The standard library is checked once per thread and reused by every later analysis whose
//! standard library would come out the same (`dz_driver::check`). Reuse is only sound if it
//! is invisible: an analysis that starts from a layer some *other* program built must
//! produce exactly what checking from scratch produces — the same diagnostics and the same
//! typed tree, standard library included.
//!
//! Each comparison runs on fresh threads, since the cache is per thread: one analyses a
//! different program first and so reuses its layer, the other analyses only this one.

use std::path::{Path, PathBuf};

use dz_driver::{analyse_with, Analysis, Mode};

fn program(name: &str) -> (PathBuf, String) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/run").join(name);
    let text = std::fs::read_to_string(root.join("src").join("main.dz")).expect("main.dz");
    (root, text)
}

fn analyse(root: &Path, text: &str, mode: Mode) -> Analysis {
    let main = Path::new("src").join("main.dz");
    let overlay = |p: &Path| p.ends_with(&main).then(|| text.to_owned());
    analyse_with(root, &overlay, mode)
}

/// What a test can see of an analysis: every diagnostic and the whole typed tree.
fn observed(a: &Analysis) -> String {
    let hir = match (&a.hir, &a.resolution) {
        (Some(hir), Some(res)) => dz_hir::dump::dump(hir, &dz_check::Names { res }),
        _ => String::new(),
    };
    format!("{:?}\n{hir}", a.diags)
}

/// A `tests/run` program, its first `{` replaced by `damage` unless that is empty.
fn damaged(name: &str, damage: &str) -> (PathBuf, String) {
    let (root, text) = program(name);
    let text = if damage.is_empty() { text } else { text.replacen('{', damage, 1) };
    (root, text)
}

/// `second` analysed after `first` on one thread, and alone on another — each a program
/// and its damage — and how many analyses on the first thread reused a layer.
fn compare(first: (&str, &str), second: (&str, &str), mode: Mode) -> (String, String, usize) {
    let (f_root, f_text) = damaged(first.0, first.1);
    let (s_root, s_text) = damaged(second.0, second.1);
    let (s_root2, s_text2) = (s_root.clone(), s_text.clone());
    let reused = std::thread::spawn(move || {
        analyse(&f_root, &f_text, mode);
        let a = analyse(&s_root, &s_text, mode);
        (observed(&a), dz_driver::prelude_reuses())
    })
    .join()
    .unwrap();
    let fresh =
        std::thread::spawn(move || observed(&analyse(&s_root2, &s_text2, mode))).join().unwrap();
    (reused.0, fresh, reused.1)
}

#[test]
fn a_reused_standard_library_checks_exactly_as_a_fresh_one() {
    // Small and large, generic-heavy, inheritance, and one that imports a header of its own.
    let pairs = [
        ("hello", "collections"),
        ("collections", "hello"),
        ("generics", "inherit"),
        ("hashmap", "slot_map"),
        ("iterator", "cinterop"),
    ];
    for (a, b) in pairs {
        for mode in [Mode::Build, Mode::Editor] {
            let (reused, fresh, reuses) = compare((a, ""), (b, ""), mode);
            assert_eq!(reuses, 1, "{b} after {a} ({mode:?}) did not reuse the standard library");
            assert!(reused == fresh, "{b} after {a} ({mode:?}) differs from {b} checked afresh");
        }
    }
}

#[test]
fn a_parse_error_reuses_only_a_layer_built_with_one() {
    // A parse error leaves every `#if` in place, standard library included, so its layer
    // is not the clean program's — and the other way round.
    let (reused, fresh, reuses) = compare(("hello", ""), ("collections", "{ ("), Mode::Editor);
    assert_eq!(reuses, 0, "a broken program reused a clean program's standard library");
    assert!(reused == fresh);

    let (reused, fresh, reuses) = compare(("hello", "{ ("), ("collections", "{ ("), Mode::Editor);
    assert_eq!(reuses, 1, "two broken programs did not share a standard library");
    assert!(reused == fresh);

    let (reused, fresh, reuses) = compare(("hello", "{ ("), ("collections", ""), Mode::Editor);
    assert_eq!(reuses, 0, "a clean program reused a broken program's standard library");
    assert!(reused == fresh);
}
