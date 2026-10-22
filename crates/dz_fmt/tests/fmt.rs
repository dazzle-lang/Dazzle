//! Formatter tests.
//!
//! "Round-trips" is pinned down as four properties, checked over every clean corpus file —
//! the formatter corpus, the parser corpus, and every whole program in `tests/run`:
//!
//! 1. **Idempotence** — `fmt(fmt(x)) == fmt(x)`.
//! 2. **Structure preservation** — formatting never changes the tree.
//! 3. **Validity** — formatted output re-parses without new errors.
//! 4. **Comment preservation** — every comment is still there, in the same order.
//!
//! Together those say the formatter only ever changes layout.

use dz_span::{SourceFile, SourceMap};

struct Run {
    map: SourceMap,
    id: dz_span::FileId,
    formatted: String,
    dump: String,
    codes: Vec<&'static str>,
    comments: Vec<String>,
}

fn run(name: &str, source: &str) -> Run {
    let mut map = SourceMap::new();
    let id = map.add(name, source.to_owned());
    let text = map.get(id).text().to_owned();
    let lexed = dz_lex::lex(id, &text);
    let parsed = dz_parse::parse(id, &text, &lexed.tokens);
    let mut codes: Vec<&'static str> = lexed.diagnostics.iter().filter_map(|d| d.code).collect();
    codes.extend(parsed.diagnostics.iter().filter_map(|d| d.code));
    let comments =
        lexed.comments.iter().map(|c| text[c.span.range()].trim_end().to_owned()).collect();
    let file: &SourceFile = map.get(id);
    let formatted = dz_fmt::format(file, &parsed.ast, &lexed.comments);
    let dump = dz_ast::dump::dump(&text, &parsed.ast);
    Run { map, id, formatted, dump, codes, comments }
}

fn fmt(source: &str) -> String {
    run("t.dz", source).formatted
}

/// Formatting `source` changes nothing, and neither does formatting it again.
#[track_caller]
fn unchanged(source: &str) {
    let r = run("t.dz", source);
    assert!(r.codes.is_empty(), "the test source itself does not parse: {:?}", r.codes);
    let out = r.formatted;
    assert_eq!(out, source, "formatting changed already-formatted source");
    assert_eq!(fmt(&out), out, "not idempotent");
}

fn corpus_files() -> Vec<std::path::PathBuf> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut out = Vec::new();
    for dir in ["tests/parse", "tests/fmt"] {
        let Ok(entries) = std::fs::read_dir(root.join(dir)) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "dz") {
                out.push(p);
            }
        }
    }
    // Whole programs: the code the formatter will actually meet. `!->` signatures and
    // comments inside `or` blocks were once mangled here while the small corpus stayed green.
    if let Ok(entries) = std::fs::read_dir(root.join("tests/run")) {
        for e in entries.flatten() {
            let main = e.path().join("src").join("main.dz");
            if main.is_file() {
                out.push(main);
            }
        }
    }
    out.sort();
    out
}

fn corpus_name(path: &std::path::Path) -> String {
    path.components()
        .rev()
        .take(3)
        .collect::<Vec<_>>()
        .iter()
        .rev()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
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
        let name = corpus_name(&path);
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
        let name = corpus_name(&path);
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
fn formatting_keeps_every_comment_in_order() {
    for path in corpus_files() {
        let name = corpus_name(&path);
        let source = std::fs::read_to_string(&path).expect("corpus file");
        let before = run(&name, &source);
        if !before.codes.is_empty() {
            continue;
        }
        let after = run(&name, &before.formatted);
        // Block comments may be re-indented inside; compare them without leading space.
        let norm = |cs: &[String]| {
            cs.iter()
                .map(|c| c.lines().map(str::trim).collect::<Vec<_>>().join("\n"))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            norm(&before.comments),
            norm(&after.comments),
            "formatting {name} lost or reordered a comment"
        );
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

/// T-0082: a comment after a `{` went to the next line, inside the block, and one after a
/// `}` to a line of its own — away from what each annotates.
#[test]
fn a_comment_after_a_brace_stays_on_its_line() {
    unchanged(
        "\
struct S { // opens
    a: i32
} // closes

impl Copy for S { // opt-in
    fn copy(&mut self, other: &Self) {
        a = other.a
    }
}

fn empty() { // nothing yet
}

fn main() { // entry
    while true { // forever
        break
    } // after the loop
    if a { // then
        foo()
    } else { // otherwise
        bar()
    }
    switch n { // on n
        case Odd { // odd
            return 1
        }
        default {
            return 2
        }
    }
    let x = halve(n) or { e -> // handle it
        0
    }
} // c, b and a are dropped
",
    );
}

/// The README's samples line their trailing comments up, across lines and across a `}`.
#[test]
fn trailing_comments_keep_the_column_they_were_written_at() {
    unchanged(
        "\
fn main() {
    let a = Image(64, 64)
    let b = a                        // a copy: runs `copy`
    let c = move(a)                  // a move: `a` is reset to its defaults
}                                    // c, b and a are dropped, in that order
",
    );
    // One space is the least, whatever the column was.
    assert_eq!(fmt("fn f() {\n    let a = 1// x\n}\n"), "fn f() {\n    let a = 1 // x\n}\n");
}

#[test]
fn a_trailing_comment_moves_with_its_re_indented_line() {
    assert_eq!(
        fmt("fn f() {\n        let a = 1      // x\n}\n"),
        "fn f() {\n    let a = 1      // x\n}\n"
    );
}

#[test]
fn an_aligned_run_of_comments_stays_aligned_when_one_line_grows() {
    // `a+1` becomes `a + 1`, which no longer leaves room at the column; the whole run moves.
    assert_eq!(
        fmt("fn f() {\n    let a = 1   // one\n    let bb = a+1// two\n}\n"),
        "fn f() {\n    let a = 1      // one\n    let bb = a + 1 // two\n}\n"
    );
}

#[test]
fn a_fallible_signature_keeps_its_bang() {
    // Spec §9.1: `!->` is part of the signature's meaning. Printing `->` made every
    // `or return` on the function an error.
    unchanged("fn halve(n: i32) !-> i32 {\n    return n / 2\n}\n");
    unchanged("struct S {\n    pub fn get(&self) !-> i32 {\n        return 1\n    }\n}\n");
}

#[test]
fn comments_stay_inside_the_blocks_they_were_written_in() {
    unchanged(
        "\
fn f(n: i32) !-> i32 {
    let handled = halve(n) or { e ->
        println!(\"{}\", e.message)
        // inside the `or` block
        0
    }
    if n > 0 {
        // inside the `if`
        println!(\"positive\")
    } else {
        // inside the `else`
        println!(\"not\")
    }
    switch n {
        case Odd {
            // inside a `case`
            return 1
        }
        default {
            return 2
        }
    }
    unsafe {
        // inside `unsafe`
        return handled
    }
}
",
    );
}

#[test]
fn a_block_holding_only_a_comment_keeps_it() {
    unchanged("fn f(n: i32) {\n    if n > 0 {\n        // nothing to do yet\n    }\n}\n");
}

#[test]
fn a_block_comment_in_a_nested_block_does_not_drift() {
    unchanged(
        "fn f(n: i32) {\n    if n > 0 {\n        /* two\n           lines */\n        println!(\"x\")\n    }\n}\n",
    );
}

#[test]
fn a_blank_line_after_a_header_comment_survives() {
    unchanged("// What this file is for.\n\nfn f() {}\n");
    unchanged("fn f() {\n    // First, the setup.\n\n    let a = 1\n}\n");
}

#[test]
fn no_blank_line_opens_a_block() {
    assert_eq!(fmt("fn f() {\n\n    let a = 1\n}\n"), "fn f() {\n    let a = 1\n}\n");
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
fn field_alignment_counts_the_visibility() {
    // T-0081: the width was the name's alone, so `pub` pushed its own fields' types right.
    unchanged(
        "struct S {\n    pub width:    u32\n    pub height:   u32\n    m_pixels:     u8\n    own m_buffer: u8\n}\n",
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

/// A macro's body and an invocation are reproduced as written, comments and all — and a
/// comment inside one is printed there once, not again after it (spec §12).
#[test]
fn a_comment_inside_a_macro_or_an_invocation_is_kept_once() {
    unchanged(
        "macro twice -> expr {\n    // doubles\n    pattern { ( $e: expr ) }\n    expand { $e * 2 }\n}\n\n\
         #machine Door() {\n    // closed first\n    state Closed {}\n}\n\n\
         fn main() {\n    trace!(\"here\") // why\n    let x = twice!(\n        // four\n        4\n    )\n}\n",
    );
}
