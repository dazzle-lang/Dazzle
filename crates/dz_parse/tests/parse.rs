//! Parser snapshot corpus and targeted grammar tests (spec §17.5).

use dz_span::SourceMap;

fn parse_report(name: &str, source: &str) -> String {
    let mut map = SourceMap::new();
    let id = map.add(name, source.to_owned());
    let file = map.get(id);

    let lexed = dz_lex::lex(id, file.text());
    let parsed = dz_parse::parse(id, file.text(), &lexed.tokens);

    let mut diags = lexed.diagnostics;
    diags.extend(parsed.diagnostics);
    diags.sort_by_position();

    let mut out = String::new();
    out.push_str("=== AST ===\n");
    out.push_str(&dz_ast::dump::dump(file.text(), &parsed.ast));
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
    insta::glob!("../../../tests/parse", "*.dz", |path| {
        let source = std::fs::read_to_string(path).expect("failed to read corpus file");
        let name = format!("tests/parse/{}", path.file_name().unwrap().to_string_lossy());
        insta::assert_snapshot!(parse_report(&name, &source));
    });
}

// --- helpers -------------------------------------------------------------

struct Parsed {
    dump: String,
    codes: Vec<&'static str>,
}

fn parse_str(source: &str) -> Parsed {
    let mut map = SourceMap::new();
    let id = map.add("t.dz", source.to_owned());
    let text = map.get(id).text().to_owned();
    let lexed = dz_lex::lex(id, &text);
    let parsed = dz_parse::parse(id, &text, &lexed.tokens);
    Parsed {
        dump: dz_ast::dump::dump(&text, &parsed.ast),
        codes: parsed.diagnostics.iter().filter_map(|d| d.code).collect(),
    }
}

/// The dump of the first expression in `fn f() { let x = <expr> }`, flattened to one line.
fn expr_shape(source: &str) -> String {
    let p = parse_str(&format!("fn f() {{ let x = {source} }}"));
    assert!(p.codes.is_empty(), "unexpected errors in {source:?}: {:?}", p.codes);
    p.dump
        .lines()
        .skip_while(|l| !l.trim_start().starts_with("let x"))
        .skip(1)
        .map(|l| l.trim())
        .collect::<Vec<_>>()
        .join(" ")
}

// --- precedence ----------------------------------------------------------

#[test]
fn arithmetic_precedence_matches_the_spec_table() {
    assert_eq!(expr_shape("1 + 2 * 3"), "binary + int 1 binary * int 2 int 3");
    assert_eq!(expr_shape("1 * 2 + 3"), "binary + binary * int 1 int 2 int 3");
    // `as` binds tighter than `*` (spec §11.1 level 3 vs 4).
    assert_eq!(expr_shape("a as i64 * b"), "binary * cast i64 path a path b");
}

#[test]
fn logical_binds_looser_than_comparison() {
    assert_eq!(
        expr_shape("a == b && c != d"),
        "binary && binary == path a path b binary != path c path d"
    );
}

#[test]
fn bitwise_precedence_is_or_xor_and() {
    // `1 | 2 ^^ 3 & 4` groups as `1 | (2 ^^ (3 & 4))`.
    assert_eq!(expr_shape("1 | 2 ^^ 3 & 4"), "binary | int 1 binary ^^ int 2 binary & int 3 int 4");
}

/// Spec §8.1: `xs.map<f32>(f)` writes a method's own type arguments, and is taken only when
/// a call follows the `>` — so a comparison after a field is still a comparison.
#[test]
fn a_method_call_may_write_type_arguments() {
    let call = expr_shape("xs.map<f32, Vec2>(f)");
    assert!(call.starts_with("call field .map<f32, Vec2> path xs"), "{call}");
    assert_eq!(expr_shape("n.len < max"), "binary < field .len path n path max");
    assert_eq!(expr_shape("(n.len < max)"), expr_shape("n.len < max"));
    assert_eq!(expr_shape("n.len < max.len"), "binary < field .len path n field .len path max");
}

#[test]
fn chained_comparison_is_rejected() {
    let p = parse_str("fn f() { let x = a < b < c }");
    assert_eq!(p.codes, vec![dz_diag::codes::CHAINED_COMPARISON]);
}

#[test]
fn chained_or_is_rejected() {
    // spec §9.4, §11.1 — `or` is non-associative. The first clause has already produced a
    // value or diverged, so a second `or` could only apply to something that cannot fail.
    let p = parse_str("fn f() { let x = a() or b() or c }");
    assert_eq!(p.codes, vec![dz_diag::codes::CHAINED_OR]);
    // One `or` is of course fine, and so is a second one inside a parenthesised operand.
    assert!(parse_str("fn f() { let x = a() or 0 }").codes.is_empty());
    assert!(parse_str("fn f() { let x = (a() or b()) or 0 }").codes.is_empty());
}

// --- the turbofish-free generic rule (spec §3.3) --------------------------

#[test]
fn generic_call_parses_without_a_turbofish() {
    assert_eq!(
        expr_shape("Buffer<f32>::with_capacity(8)"),
        "call path Buffer<f32>::with_capacity int 8"
    );
}

#[test]
fn less_than_still_parses_as_comparison() {
    assert_eq!(expr_shape("a < b"), "binary < path a path b");
    assert_eq!(
        expr_shape("(a < b) && (c > d)"),
        "binary && binary < path a path b binary > path c path d"
    );
}

#[test]
fn speculative_generic_parse_leaves_no_stray_diagnostics() {
    // The `<` here is a comparison; rolling back must not leak the type-parse errors.
    let p = parse_str("fn f() { let x = count < limit }");
    assert!(p.codes.is_empty(), "leaked diagnostics: {:?}", p.codes);
}

// --- pointers and references ---------------------------------------------

#[test]
fn caret_is_deref_in_postfix_position() {
    assert_eq!(expr_shape("p^"), "deref path p");
    assert_eq!(expr_shape("a ^^ b"), "binary ^^ path a path b");
}

#[test]
fn reference_types_parse() {
    let p =
        parse_str("fn f(a: &T, b: &mut T, c: ^T, d: own ^T, e: T?, g: ^T?, h: []T, i: [4]T) {}");
    assert!(p.codes.is_empty(), "{:?}", p.codes);
    assert!(p.dump.contains("a: &T"), "{}", p.dump);
    assert!(p.dump.contains("b: &mut T"), "{}", p.dump);
    assert!(p.dump.contains("d: own ^T"), "{}", p.dump);
    assert!(p.dump.contains("e: T?"), "{}", p.dump);
    // `?` is postfix and binds to the type name, so access sigils sit outside it: this is
    // a pointer to an optional, not an optional pointer (spec §5.6).
    assert!(p.dump.contains("g: ^T?"), "{}", p.dump);
}

#[test]
fn a_fallible_signature_is_marked_on_the_arrow() {
    // spec §9 — `!` never appears in a type, so there is no `T!` to parse.
    let p = parse_str("fn f() !-> Header { return h }");
    assert!(p.codes.is_empty(), "{:?}", p.codes);
    assert!(p.dump.contains("fn f() !-> Header"), "{}", p.dump);
}

// --- struct literal ambiguity --------------------------------------------

#[test]
fn braces_after_a_condition_start_a_block_not_a_struct_literal() {
    // `if ready { ... }` must not parse `ready { ... }` as a struct literal.
    let p = parse_str("fn f() { if ready { work() } }");
    assert!(p.codes.is_empty(), "{:?}", p.codes);
    assert!(p.dump.contains("if"), "{}", p.dump);
    assert!(!p.dump.contains("struct-lit"), "{}", p.dump);

    // Inside parentheses a struct literal is legal again.
    let p2 = parse_str("fn f() { if check(Vec3{ 1, 2, 3 }) { work() } }");
    assert!(p2.codes.is_empty(), "{:?}", p2.codes);
    assert!(p2.dump.contains("struct-lit Vec3"), "{}", p2.dump);
}

// --- recovery ------------------------------------------------------------

#[test]
fn a_broken_declaration_does_not_swallow_the_next_one() {
    let p = parse_str("fn broken( {\n}\nfn good() -> i32 {\n    return 7\n}\n");
    assert!(!p.codes.is_empty(), "expected an error");
    assert!(p.dump.contains("fn good"), "recovery lost the next declaration:\n{}", p.dump);
}

#[test]
fn parsing_always_terminates_on_hostile_input() {
    for src in ["fn", "struct {", "fn f() { let", "((((", "}}}}", "fn f() { switch", "#"] {
        let p = parse_str(src);
        // The point is that this returns at all; the tree may be mostly errors.
        let _ = p.dump;
    }
}

// --- struct inheritance (spec §7.3, 18.32) -------------------------------

#[test]
fn a_base_and_its_initialiser_parse() {
    let p = parse_str("struct Button : UiElement {\n Button(w: i32) : UiElement(w) {}\n}");
    assert!(p.codes.is_empty(), "{:?}", p.codes);
    assert!(p.dump.contains("struct Button : UiElement"), "{}", p.dump);
    // The dump records the base initialiser and how many arguments it took, so a
    // constructor that silently lost one would fail here rather than at runtime.
    assert!(p.dump.contains("ctor Button(w: i32) : UiElement/1"), "{}", p.dump);
}

#[test]
fn virtual_and_override_parse_and_a_bodyless_virtual_is_allowed() {
    let p = parse_str(
        "struct S {\n virtual fn a(&self) -> i32\n override fn b(&self) {}\n \
         virtual unsafe fn c(&self) {}\n}",
    );
    assert!(p.codes.is_empty(), "{:?}", p.codes);
    assert!(p.dump.contains("virtual fn a(&self) -> i32"), "{}", p.dump);
    assert!(p.dump.contains("override fn b(&self)"), "{}", p.dump);
    assert!(p.dump.contains("virtual unsafe fn c(&self)"), "{}", p.dump);
}

#[test]
fn protected_is_a_member_visibility_only() {
    // spec §4: it means "and the types that inherit from this one", which only a member
    // of a type can mean — at module scope there is nothing to inherit from.
    let ok = parse_str("struct S {\n protected n: i32\n protected fn m(&self) {}\n}");
    assert!(ok.codes.is_empty(), "{:?}", ok.codes);
    assert!(ok.dump.contains("protected fn m(&self)"), "{}", ok.dump);

    let bad = parse_str("protected fn f() {}");
    assert!(
        bad.codes.contains(&dz_diag::codes::EXPECTED_TOKEN),
        "expected a diagnostic, got {:?}",
        bad.codes
    );
}

#[test]
fn every_corpus_file_that_should_be_clean_is_clean() {
    for name in ["items.dz", "expressions.dz", "statements.dz"] {
        let path = concat_path(name);
        let source = std::fs::read_to_string(&path).expect("corpus file");
        let mut map = SourceMap::new();
        let id = map.add(name, source);
        let text = map.get(id).text().to_owned();
        let lexed = dz_lex::lex(id, &text);
        let parsed = dz_parse::parse(id, &text, &lexed.tokens);
        let codes: Vec<&str> = parsed.diagnostics.iter().filter_map(|d| d.code).collect();
        assert!(codes.is_empty(), "{name} should parse cleanly, got {codes:?}");
    }
}

fn concat_path(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/parse").join(name)
}

// --- ties and kills clauses (spec §6.7) -----------------------------------

/// In a function type, a comma after a bare path begins the next parameter: more than one
/// path needs parentheses there, and a declaration's list does not.
#[test]
fn a_comma_after_a_kills_path_in_a_function_type_begins_the_next_parameter() {
    let p = parse_str("fn each(f: fn(xs: &mut List<i32>) kills xs@items, n: i32) {}\n");
    assert!(p.codes.is_empty(), "{:?}", p.codes);
    assert!(
        p.dump.contains("fn each(f: fn(xs: &mut List<i32>) kills xs@items, n: i32)"),
        "{}",
        p.dump
    );
    let p = parse_str("fn f(a: &mut A, b: &mut B) kills a@x, b@y {}\n");
    assert!(p.codes.is_empty(), "{:?}", p.codes);
    assert!(p.dump.contains("kills a@x, b@y"), "{}", p.dump);
}

/// `kills` is a keyword, so it can no longer name anything (spec §2.7).
#[test]
fn kills_is_not_a_name() {
    assert!(!parse_str("fn f() { let kills = 1 }\n").codes.is_empty());
}
