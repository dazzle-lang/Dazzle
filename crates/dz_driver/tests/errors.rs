//! The error corpus: `tests/errors/*.dz`, one small program per mistake.
//!
//! Each file names the diagnostics it must produce in `// expect:` lines at its top —
//! `// expect: DZ0548`, `// expect: DZ0549, DZ0549` for two, `// expect: (none)` for a
//! program that must be accepted. The match is **exact**: the same codes, as many times as
//! listed, so one mistake reported twice fails (invariant 6), and so does a mistake that
//! also drags in an unrelated error.
//!
//! This is where rules are tested *in combination* — a `switch` inside a `for` inside a
//! generic function, a binding written through inside a `case` — because that is where
//! bugs have been found: each feature had a test of its own and still broke when used
//! with another. The whole pipeline runs (resolution, checking, ownership), as `dz check`
//! does, so a file tests what a user would see.

use std::path::{Path, PathBuf};

fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/errors")
}

/// The codes an `// expect:` header asks for, in order of appearance.
fn expected(source: &str) -> Vec<String> {
    source
        .lines()
        .take_while(|l| l.starts_with("//") || l.trim().is_empty())
        .filter_map(|l| l.strip_prefix("// expect:"))
        .flat_map(|l| l.split(',').map(|c| c.trim().to_owned()))
        .filter(|c| !c.is_empty() && c != "(none)")
        .collect()
}

/// Every code `dz check` reports for one file, as a program of its own.
fn run(path: &Path) -> (Vec<String>, String) {
    let stem = path.file_stem().unwrap().to_string_lossy();
    let dir = std::env::temp_dir().join(format!("dz_errors_{stem}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("dazzle.toml"),
        "[module]\nname = \"errors\"\nsources = [\"src/**/*.dz\"]\n\n[dependencies]\nstd = { open = true }\n",
    )
    .unwrap();
    std::fs::copy(path, dir.join("src/main.dz")).unwrap();
    let analysis = dz_driver::analyse(&dir);
    let codes = analysis.diags.iter().filter_map(|d| d.code).map(str::to_owned).collect();
    let rendered = dz_diag::render_to_string(&analysis.map, &analysis.diags);
    let _ = std::fs::remove_dir_all(&dir);
    (codes, rendered)
}

#[test]
fn every_file_produces_exactly_what_it_expects() {
    let mut files: Vec<PathBuf> = std::fs::read_dir(corpus())
        .expect("tests/errors exists")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "dz"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "the error corpus is empty");

    let mut failures = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for path in &files {
        let source = std::fs::read_to_string(path).unwrap();
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let mut want = expected(&source);
        assert!(
            source.lines().any(|l| l.starts_with("// expect:")),
            "{name}: no `// expect:` line — write `// expect: (none)` for a valid program"
        );
        let (mut got, rendered) = run(path);
        // An unused binding is a warning about the fixture, not the rule under test, unless
        // the file is about exactly that.
        if !want.iter().any(|c| c == UNUSED_VARIABLE) {
            got.retain(|c| c != UNUSED_VARIABLE);
        }
        seen.extend(got.iter().cloned());
        want.sort();
        got.sort();
        if want != got {
            failures.push(format!("{name}: expected {want:?}, got {got:?}\n{rendered}"));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} files failed:\n\n{}",
        failures.len(),
        files.len(),
        failures.join("\n")
    );

    // The rules added with the corpus have to stay covered.
    use dz_diag::codes::*;
    for code in [BAD_CASE_LABEL, DUPLICATE_CASE, BAD_BINDING, NOT_MUTABLE, IMPLICIT_COPY] {
        assert!(seen.iter().any(|s| s == code), "no tests/errors case produces {code}");
    }
}
