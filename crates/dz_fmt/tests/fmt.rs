//! Formatter tests.
//!
//! "Round-trips" is pinned down as three properties, checked over every clean corpus file:
//!
//! 1. **Idempotence** — `fmt(fmt(x)) == fmt(x)`.
//! 2. **Structure preservation** — formatting never changes the tree.
//! 3. **Validity** — formatted output re-parses without new errors.
//!
//! Together those say the formatter only ever changes layout.

use dz_span::{SourceFile, SourceMap};

struct Run {
    map: SourceMap,
    id: dz_span::FileId,
    formatted: String,
    dump: String,
    codes: Vec<&'static str>,
}

fn run(name: &str, source: &str) -> Run {
    let mut map = SourceMap::new();
    let id = map.add(name, source.to_owned());
    let text = map.get(id).text().to_owned();
    let lexed = dz_lex::lex(id, &text);
    let parsed = dz_parse::parse(id, &text, &lexed.tokens);
    let mut codes: Vec<&'static str> = lexed.diagnostics.iter().filter_map(|d| d.code).collect();
    codes.extend(parsed.diagnostics.iter().filter_map(|d| d.code));
    let file: &SourceFile = map.get(id);
    let formatted = dz_fmt::format(file, &parsed.ast, &lexed.comments);
    let dump = dz_ast::dump::dump(&text, &parsed.ast);
    Run { map, id, formatted, dump, codes }
}

fn fmt(source: &str) -> String {
    run("t.dz", source).formatted
}

fn corpus_files() -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for dir in ["../../tests/parse", "../../tests/fmt"] {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(dir);
        let Ok(entries) = std::fs::read_dir(&root) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "dz") {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

#[test]
fn snapshot_corpus() {
    insta::glob!("../../../tests/fmt", "*.dz", |path| {
        let source = std::fs::read_to_string(path).expect("corpus file");
        insta::assert_snapshot!(fmt(&source));
    });
}

#[test]
fn formatting_is_idempotent() {
    for path in corpus_files() {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let source = std::fs::read_to_string(&path).expect("corpus file");
        let first = run(&name, &source);
        if !first.codes.is_empty() {
            continue; // files that deliberately contain errors are not formatted
        }
        let second = fmt(&first.formatted);
        assert_eq!(
            first.formatted, second,
            "formatting {name} is not idempotent; second pass differs"
        );
    }
}

#[test]
fn formatting_preserves_the_tree() {
    for path in corpus_files() {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let source = std::fs::read_to_string(&path).expect("corpus file");
        let before = run(&name, &source);
        if !before.codes.is_empty() {
            continue;
        }
        let after = run(&name, &before.formatted);
        assert!(
            after.codes.is_empty(),
            "formatted {name} no longer parses cleanly: {:?}",
            after.codes
        );
        assert_eq!(before.dump, after.dump, "formatting {name} changed the syntax tree");
        let _ = (&before.map, before.id);
    }
}

#[test]
fn comments_survive_formatting() {
    let src = "\
// leading comment
fn f() {
    // inside
    let a = 1 // trailing
    /* block */
    let b = 2
}
";
    let out = fmt(src);
    for expected in ["// leading comment", "// inside", "// trailing", "/* block */"] {
        assert!(out.contains(expected), "lost {expected:?}:\n{out}");
    }
}

#[test]
fn doc_comments_stay_attached() {
    let out = fmt("/// docs\npub fn f() {}\n");
    assert!(out.starts_with("/// docs\npub fn f()"), "{out}");
}

#[test]
fn blank_lines_between_declarations_collapse_to_one() {
    let out = fmt("fn a() {}\n\n\n\n\nfn b() {}\n");
    assert_eq!(out, "fn a() {}\n\nfn b() {}\n");
}

#[test]
fn parentheses_are_reinserted_where_precedence_needs_them() {
    assert_eq!(fmt("fn f() { let x = (1 + 2) * 3 }"), "fn f() {\n    let x = (1 + 2) * 3\n}\n");
    // ...and dropped where they are redundant.
    assert_eq!(fmt("fn f() { let x = 1 + (2 * 3) }"), "fn f() {\n    let x = 1 + 2 * 3\n}\n");
}

#[test]
fn consecutive_fields_get_their_types_aligned() {
    let out = fmt("struct S {\nab: i32\nlonger_name: f32\nc: u8\n}\n");
    assert_eq!(
        out,
        "struct S {\n    ab:          i32\n    longer_name: f32\n    c:           u8\n}\n"
    );
}

#[test]
fn long_parameter_lists_break_one_per_line() {
    let src = "fn f(alpha: SomeVeryLongTypeName, beta: AnotherRatherLongTypeName, gamma: AndAThirdLongTypeName, delta: PlusOneMore) {}\n";
    let out = fmt(src);
    assert!(out.contains("fn f(\n"), "expected a broken parameter list:\n{out}");
    // and it must still be idempotent
    assert_eq!(fmt(&out), out);
}

#[test]
fn source_with_errors_is_reported_not_reformatted() {
    // The driver refuses to format broken files; this pins the signal it relies on.
    let r = run("t.dz", "fn broken( {\n}\n");
    assert!(!r.codes.is_empty());
}
