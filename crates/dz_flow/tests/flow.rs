//! Ownership tests (spec §6, §17.5).
//!
//! One file per rule in `tests/flow/`, each naming the code it must produce in an
//! `// expect:` line. A regression then names the rule it broke instead of producing a
//! large diff, and `clean.dz` is the other half of the pair: an analysis that rejects
//! valid code is as broken as one that accepts invalid code.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use dz_diag::codes::UNUSED_VARIABLE;
use dz_diag::Diagnostics;
use dz_project::ModuleId;
use dz_span::SourceMap;

struct Outcome {
    codes: Vec<&'static str>,
    rendered: String,
}

impl Outcome {
    /// The codes, less the unused-binding warning. A fixture for one rule — or `typed/`,
    /// which declares values only to type-check them — has throwaway bindings, and a
    /// warning about them says nothing about the rule under test.
    fn without_unused(&self) -> Vec<&'static str> {
        self.codes.iter().copied().filter(|c| *c != UNUSED_VARIABLE).collect()
    }
}

fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/flow")
}

/// Run the pipeline through flow analysis on one source file.
fn run(source_path: &Path) -> Outcome {
    let dir = std::env::temp_dir().join(format!(
        "dz_flow_{}",
        source_path.file_stem().unwrap().to_string_lossy()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("dazzle.toml"),
        "[module]\nname = \"flow\"\nsources = [\"src/**/*.dz\"]\n",
    )
    .unwrap();
    std::fs::copy(source_path, dir.join("src/main.dz")).unwrap();
    let outcome = analyse(&dir);
    let _ = std::fs::remove_dir_all(&dir);
    outcome
}

fn analyse(dir: &Path) -> Outcome {
    let dir = dir.to_path_buf();
    let mut map = SourceMap::new();
    let mut diags = Diagnostics::new();
    let ws = dz_project::load(&dir, &mut map, &mut diags).expect("fixture manifest");

    let mut files: HashMap<ModuleId, Vec<dz_resolve::ParsedFile>> = HashMap::new();
    for module in ws.modules() {
        let mut parsed = Vec::new();
        for path in &module.sources {
            let bytes = module.read_source(path).expect("fixture source");
            let id = map.add_bytes(path, &bytes).expect("fixture is UTF-8");
            let text = map.get(id).text().to_owned();
            let lexed = dz_lex::lex(id, &text);
            let p = dz_parse::parse(id, &text, &lexed.tokens);
            diags.extend(lexed.diagnostics);
            diags.extend(p.diagnostics);
            parsed.push(dz_resolve::ParsedFile { file: id, ast: p.ast });
        }
        files.insert(module.id, parsed);
    }
    assert!(
        !diags.has_errors(),
        "fixture should lex and parse cleanly:\n{}",
        dz_diag::render_to_string(&map, &diags)
    );

    let resolution = dz_resolve::resolve(&ws, &files);
    diags.extend(resolution.diagnostics.clone());
    let checked = dz_check::check(&ws, &resolution, &files);
    diags.extend(checked.diagnostics.clone());
    if !checked.diagnostics.has_errors() {
        diags.extend(dz_flow::check(&checked.hir, &checked.table, &resolution));
    }
    diags.sort_by_position();

    let rendered = dz_diag::render_to_string(&map, &diags);
    Outcome {
        codes: diags.iter().filter_map(|d| d.code).collect(),
        rendered,
    }
}

/// The `typed/` fixture is the checker's "valid code is accepted" half (invariant 8), and
/// ownership has to hold it to the same standard: a `move` that a correct program makes is
/// not an error, and a pass that says otherwise is as broken as one that misses a real one.
#[test]
fn the_typed_workspace_is_ownership_clean() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/workspace/typed");
    let got = analyse(&dir);
    assert!(
        got.without_unused().is_empty(),
        "valid code produced ownership diagnostics: {:?}\n{}",
        got.codes,
        got.rendered
    );
}

/// The codes an `// expect:` header asks for. `(none)` means the file must be clean.
fn expected(source: &str) -> Vec<String> {
    source
        .lines()
        .take_while(|l| l.starts_with("//") || l.trim().is_empty())
        .filter_map(|l| l.strip_prefix("// expect:"))
        .flat_map(|l| l.split(',').map(|c| c.trim().to_owned()))
        .filter(|c| c != "(none)")
        .collect()
}

#[test]
fn every_ownership_rule_has_a_case() {
    let mut seen: Vec<String> = Vec::new();
    let mut files: Vec<PathBuf> = std::fs::read_dir(corpus())
        .expect("tests/flow exists")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "dz"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "the flow corpus is empty");

    for path in &files {
        let source = std::fs::read_to_string(path).unwrap();
        let want = expected(&source);
        let outcome = run(path);
        // Unused-binding warnings count only in a fixture that asks for them.
        let got = if want.iter().any(|c| c == UNUSED_VARIABLE) {
            outcome.codes.clone()
        } else {
            outcome.without_unused()
        };
        let name = path.file_name().unwrap().to_string_lossy().to_string();

        for code in &want {
            assert!(
                got.iter().any(|c| c == code),
                "{name}: expected {code}, got {:?}\n{}",
                got,
                outcome.rendered
            );
            seen.push(code.clone());
        }
        if want.is_empty() {
            assert!(
                got.is_empty(),
                "{name}: expected no diagnostics, got {:?}\n{}",
                got,
                outcome.rendered
            );
        } else {
            // One mistake, one diagnostic (invariant 6 in CLAUDE.md). A rule that fires
            // twice for one violation is a regression even though the assertion above
            // would still pass.
            assert_eq!(
                got.len(),
                want.len(),
                "{name}: expected exactly {:?}, got {:?}\n{}",
                want,
                got,
                outcome.rendered
            );
        }
    }

    // Every code this pass can produce needs a file, or the corpus quietly stops
    // covering a rule that still exists.
    use dz_diag::codes::*;
    for code in [
        USE_AFTER_MOVE,
        MOVE_OUT_OF_BORROW,
        IMPLICIT_COPY,
        USE_OF_PARTIAL_MOVE,
        MISSING_RETURN,
        UNUSED_VARIABLE,
        DUPLICATE_SPECIAL_MEMBER,
        OWN_FIELD_WITHOUT_DROP,
    ] {
        assert!(seen.iter().any(|s| s == code), "no tests/flow case produces {code}");
    }
}
