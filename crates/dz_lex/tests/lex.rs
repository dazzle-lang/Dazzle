//! Snapshot tests over the corpus in `tests/lex/` at the repository root (spec §17.5).
//!
//! Each `.dz` file produces one snapshot holding the token stream and the rendered
//! diagnostics. Review changes with `cargo insta review`.

use dz_span::SourceMap;

/// Lex one source file and render tokens plus diagnostics as reviewable text.
fn lex_report(name: &str, source: &str) -> String {
    let mut map = SourceMap::new();
    let id = map.add(name, source.to_owned());
    let file = map.get(id);

    let result = dz_lex::lex(id, file.text());
    let mut diags = result.diagnostics;
    diags.sort_by_position();

    let mut out = String::new();
    out.push_str("=== TOKENS ===\n");
    out.push_str(&dz_lex::dump_tokens(file, &result.tokens));
    out.push_str("\n=== DIAGNOSTICS ===\n");
    if diags.is_empty() {
        out.push_str("(none)\n");
    } else {
        out.push_str(&dz_diag::render_to_string(&map, &diags));
    }
    out
}

#[test]
fn corpus() {
    insta::glob!("../../../tests/lex", "*.dz", |path| {
        let bytes = std::fs::read(path).expect("failed to read corpus file");
        let name = format!(
            "tests/lex/{}",
            path.file_name().unwrap().to_string_lossy()
        );

        let mut map = SourceMap::new();
        let source = match map.add_bytes(&name, &bytes) {
            Ok(id) => map.get(id).text().to_owned(),
            Err(e) => {
                insta::assert_snapshot!(format!("=== ENCODING REJECTED ===\n{e}\n"));
                return;
            }
        };
        insta::assert_snapshot!(lex_report(&name, &source));
    });
}

// --- targeted unit tests -------------------------------------------------
//
// The corpus covers breadth. These pin down individual §2 rules so a regression names the
// rule it broke instead of showing a large snapshot diff.

use dz_lex::TokenKind;

fn kinds(source: &str) -> Vec<TokenKind> {
    let mut map = SourceMap::new();
    let id = map.add("t.dz", source.to_owned());
    dz_lex::lex(id, map.get(id).text())
        .tokens
        .iter()
        .map(|t| t.kind)
        .collect()
}

fn error_codes(source: &str) -> Vec<&'static str> {
    let mut map = SourceMap::new();
    let id = map.add("t.dz", source.to_owned());
    dz_lex::lex(id, map.get(id).text())
        .diagnostics
        .iter()
        .filter_map(|d| d.code)
        .collect()
}

#[test]
fn caret_is_a_pointer_and_xor_is_doubled() {
    use TokenKind::*;
    assert_eq!(kinds("^ ^^"), vec![Caret, CaretCaret, Eof]);
}

#[test]
fn range_wins_over_float_when_no_digit_follows_the_dot() {
    use TokenKind::*;
    assert_eq!(kinds("0..10"), vec![Int, DotDot, Int, Eof]);
    assert_eq!(kinds("0..=10"), vec![Int, DotDotEq, Int, Eof]);
    // `4.hello` is a field access, not a malformed float.
    assert_eq!(kinds("4.hello"), vec![Int, Dot, Ident, Eof]);
    assert!(error_codes("0..10").is_empty());
}

#[test]
fn multiple_dots_are_one_error_over_the_whole_literal() {
    assert_eq!(error_codes("1.2.3.466"), vec![dz_diag::codes::INVALID_NUMBER]);
    assert_eq!(kinds("1.2.3.466"), vec![TokenKind::Float, TokenKind::Eof]);
}

#[test]
fn hex_digits_are_not_mistaken_for_suffixes() {
    // `f` is a hex digit here, not an f32 suffix.
    assert!(error_codes("0xff").is_empty());
    assert!(error_codes("0xFFu32").is_empty());
    assert_eq!(error_codes("0b102"), vec![dz_diag::codes::INVALID_DIGIT]);
}

#[test]
fn newlines_terminate_but_operators_suppress() {
    use TokenKind::*;
    // A newline after `=` does not terminate the statement.
    assert_eq!(kinds("let a =\n1"), vec![Let, Ident, Eq, Int, Eof]);
    // A newline between two statements does.
    assert_eq!(kinds("a\nb"), vec![Ident, Terminator, Ident, Eof]);
    // Consecutive terminators collapse, and a leading one is dropped.
    assert_eq!(kinds("\n\n;;a"), vec![Ident, Eof]);
    // A terminator immediately before a closing bracket is not one.
    assert_eq!(kinds("f(a,\nb\n)"), vec![Ident, LParen, Ident, Comma, Ident, RParen, Eof]);
}

#[test]
fn block_comments_nest() {
    assert!(error_codes("/* a /* b */ c */").is_empty());
    assert_eq!(
        error_codes("/* a /* b */"),
        vec![dz_diag::codes::UNTERMINATED_BLOCK_COMMENT]
    );
}

#[test]
fn doc_comments_are_kept_but_plain_comments_are_not() {
    use TokenKind::*;
    assert_eq!(kinds("/// doc\na"), vec![DocComment, Terminator, Ident, Eof]);
    assert_eq!(kinds("// plain\na"), vec![Ident, Eof]);
    assert_eq!(kinds("//// plain\na"), vec![Ident, Eof]);
}

#[test]
fn unterminated_string_recovers_at_the_line_break() {
    use TokenKind::*;
    // The rest of the file must still lex — this is the bug in examples/main.dz.
    assert_eq!(kinds("\"oops\nlet a = 1"), vec![Str, Terminator, Let, Ident, Eq, Int, Eof]);
    assert_eq!(error_codes("\"oops\nlet a = 1"), vec![dz_diag::codes::UNTERMINATED_STRING]);
}

#[test]
fn spans_index_the_original_bytes_under_crlf() {
    let source = "let a = 1\r\nlet b = 2\r\n";
    let mut map = SourceMap::new();
    let id = map.add("t.dz", source.to_owned());
    let file = map.get(id);
    let tokens = dz_lex::lex(id, file.text()).tokens;

    // The second `let` must report line 2, column 1, and its span must slice to "let".
    let second_let = tokens.iter().filter(|t| t.kind == TokenKind::Let).nth(1).unwrap();
    let loc = file.location(second_let.span.start as usize);
    assert_eq!((loc.line, loc.column), (2, 1));
    assert_eq!(second_let.text(file.text()), "let");
}

#[test]
fn keywords_are_distinguished_from_identifiers() {
    use TokenKind::*;
    assert_eq!(kinds("let letx"), vec![Let, Ident, Eof]);
    assert_eq!(kinds("self Self"), vec![SelfValue, SelfType, Eof]);
    // Reserved words still lex as identifiers for now (spec §2.7).
    assert_eq!(kinds("unsafe"), vec![Ident, Eof]);
}

#[test]
fn every_input_ends_with_exactly_one_eof() {
    for src in ["", "\n", "a", "\"unterminated", "/* unterminated", "0x"] {
        let k = kinds(src);
        assert_eq!(k.last(), Some(&TokenKind::Eof), "input {src:?}");
        assert_eq!(
            k.iter().filter(|k| **k == TokenKind::Eof).count(),
            1,
            "input {src:?}"
        );
    }
}
