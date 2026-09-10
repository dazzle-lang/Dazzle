//! Type-checker tests (spec §5, §7, §9, §10, §11).
//!
//! Two fixtures carry the weight, and they are deliberately a matched pair: `typed/` must
//! produce **no** diagnostics, `typeerrors/` must produce one for each rule. A checker that
//! rejects valid code is as broken as one that accepts invalid code, and only the pair
//! catches both failures.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use dz_diag::Diagnostics;
use dz_project::ModuleId;
use dz_span::SourceMap;

struct Checked {
    rendered: String,
    codes: Vec<&'static str>,
    error_count: usize,
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/workspace").join(name)
}

/// Run the whole pipeline: manifest, lex, parse, resolve, type-check.
fn check(entry: &Path) -> Checked {
    let mut map = SourceMap::new();
    let mut diags = Diagnostics::new();

    let Some(ws) = dz_project::load(entry, &mut map, &mut diags) else {
        return Checked { rendered: "failed to load workspace".into(), codes: vec![], error_count: 1 };
    };

    let mut files: HashMap<ModuleId, Vec<dz_resolve::ParsedFile>> = HashMap::new();
    for module in ws.modules() {
        let mut parsed = Vec::new();
        for path in &module.sources {
            let bytes = std::fs::read(path).expect("fixture source");
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
    assert!(!diags.has_errors(), "fixture should lex and parse cleanly");

    let resolution = dz_resolve::resolve(&ws, &files);
    let resolve_failed = resolution.diagnostics.has_errors();
    diags.extend(resolution.diagnostics.clone());
    if !resolve_failed {
        diags.extend(dz_check::check(&ws, &resolution, &files).diagnostics);
    }
    diags.sort_by_position();

    let root = fixture("").to_string_lossy().replace('\\', "/");
    let rendered = dz_diag::render_to_string(&map, &diags).replace(&root, "").replace('\\', "/");

    Checked {
        codes: diags.iter().filter_map(|d| d.code).collect(),
        error_count: diags.error_count(),
        rendered,
    }
}

/// Type-check a snippet as a one-file module.
fn snippet(source: &str) -> Vec<&'static str> {
    let dir = std::env::temp_dir().join(format!("dz_check_{:x}", hash(source)));
    let _ = std::fs::create_dir_all(dir.join("src"));
    std::fs::write(dir.join("dazzle.toml"), "[module]\nname = \"t\"\nsources = [\"src/**/*.dz\"]\n")
        .unwrap();
    std::fs::write(dir.join("src/a.dz"), source).unwrap();
    let r = check(&dir);
    let _ = std::fs::remove_dir_all(&dir);
    r.codes
}

fn hash(s: &str) -> u64 {
    let mut h = 0xcbf29ce484222325u64;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

// --- the milestone criterion ---------------------------------------------

#[test]
fn a_non_generic_program_type_checks() {
    let r = check(&fixture("typed"));
    assert_eq!(r.error_count, 0, "valid code was rejected:\n{}", r.rendered);
    assert!(r.codes.is_empty(), "valid code produced diagnostics:\n{}", r.rendered);
}

#[test]
fn the_m2_workspace_still_checks_clean() {
    let r = check(&fixture("good"));
    assert_eq!(r.error_count, 0, "{}", r.rendered);
}

#[test]
fn every_rule_has_a_failing_case() {
    let r = check(&fixture("typeerrors"));
    use dz_diag::codes::*;
    for expected in [
        TYPE_MISMATCH,
        LITERAL_OUT_OF_RANGE,
        WRONG_ARG_COUNT,
        NO_SUCH_FIELD,
        PRIVATE_FIELD,
        NOT_MUTABLE,
        CONDITION_NOT_BOOL,
        UNUSED_RESULT,
        INVALID_CAST,
        MISSING_FIELD,
        NON_EXHAUSTIVE_SWITCH,
        INVALID_OPERANDS,
        NOT_INDEXABLE,
        TRY_OUTSIDE_FALLIBLE,
        REF_IN_FIELD,
        NOT_DEREFERENCEABLE,
        NO_SUCH_METHOD,
        NO_CONTEXT_FOR_LITERAL,
    ] {
        assert!(r.codes.contains(&expected), "expected {expected} in:\n{}", r.rendered);
    }
}

#[test]
fn diagnostics_snapshot() {
    for name in ["typed", "typeerrors"] {
        let r = check(&fixture(name));
        let body = if r.rendered.is_empty() { "(none)\n".to_owned() } else { r.rendered };
        insta::assert_snapshot!(format!("check_{name}"), body);
    }
}

// --- targeted rules -------------------------------------------------------

#[test]
fn there_are_no_implicit_numeric_conversions() {
    // Not even widening (spec §5.1).
    assert!(snippet("fn f(a: i32) -> i64 {\n    return a\n}\n")
        .contains(&dz_diag::codes::TYPE_MISMATCH));
    assert!(snippet("fn f(a: i32) -> i64 {\n    return a as i64\n}\n").is_empty());
    // Mixing widths in arithmetic is rejected too.
    assert!(snippet("fn f(a: i32, b: i64) -> i64 {\n    return a + b\n}\n")
        .contains(&dz_diag::codes::INVALID_OPERANDS));
}

#[test]
fn untyped_literals_take_the_type_of_their_context() {
    // spec §2.5 — none of these is a cast.
    assert!(snippet("fn f() {\n let a: u64 = 5\n let b: f32 = 1\n let c: i8 = -3\n}\n").is_empty());
    // ...but the value still has to fit.
    assert!(snippet("fn f() {\n let b: u8 = 300\n}\n")
        .contains(&dz_diag::codes::LITERAL_OUT_OF_RANGE));
}

#[test]
fn an_unannotated_float_literal_is_f64() {
    // spec §18.1 — the default only applies where no context types the literal.
    assert!(snippet("fn f(v: f32) {\n let total = 0.0\n let x = total + v\n}\n")
        .contains(&dz_diag::codes::INVALID_OPERANDS));
    assert!(snippet("fn f(v: f32) {\n let total: f32 = 0\n let x = total + v\n}\n").is_empty());
}

#[test]
fn conditions_must_be_bool() {
    // There is no truthiness (spec §10.1).
    assert!(snippet("fn f(n: i32) {\n if n {\n  return\n }\n}\n")
        .contains(&dz_diag::codes::CONDITION_NOT_BOOL));
    assert!(snippet("fn f(n: i32) {\n if n != 0 {\n  return\n }\n}\n").is_empty());
}

#[test]
fn mutation_requires_a_mutable_binding_or_receiver() {
    // spec §7.3
    assert!(snippet("fn f() {\n let a = 1\n a = 2\n}\n").contains(&dz_diag::codes::NOT_MUTABLE));
    assert!(snippet("fn f() {\n let mut a = 1\n a = 2\n}\n").is_empty());
    // A `fn` method takes `&self`, so it cannot assign to a field.
    assert!(snippet(
        "struct S {\n n: i32\n pub fn bad(&self) {\n  n = 1\n }\n}\n"
    )
    .contains(&dz_diag::codes::NOT_MUTABLE));
    assert!(snippet(
        "struct S {\n n: i32\n pub mut fn ok(&mut self) {\n  n = 1\n }\n}\n"
    )
    .is_empty());
}

#[test]
fn switch_must_cover_every_variant() {
    // spec §10.3
    let missing = "union U {\n A\n B\n}\nfn f(u: U) -> i32 {\n switch u {\n  case A {\n   return 1\n  }\n }\n return 0\n}\n";
    assert!(snippet(missing).contains(&dz_diag::codes::NON_EXHAUSTIVE_SWITCH));
    let covered = "union U {\n A\n B\n}\nfn f(u: U) -> i32 {\n switch u {\n  case A {\n   return 1\n  }\n  case B {\n   return 2\n  }\n }\n return 0\n}\n";
    assert!(snippet(covered).is_empty());
}

#[test]
fn a_fallible_result_cannot_be_dropped_on_the_floor() {
    // spec §9.7
    let base = "fn may_fail() -> !i32 {\n return 1\n}\n";
    assert!(snippet(&format!("{base}fn g() {{\n may_fail()\n}}\n"))
        .contains(&dz_diag::codes::UNUSED_RESULT));
    // Each of the four sanctioned ways to consume it is accepted.
    assert!(snippet(&format!("{base}fn g() {{\n let _ = may_fail()\n}}\n")).is_empty());
    assert!(snippet(&format!("{base}fn g() -> !i32 {{\n let v = try may_fail()\n return v\n}}\n")).is_empty());
    assert!(snippet(&format!("{base}fn g() -> i32 {{\n return may_fail() catch 0\n}}\n")).is_empty());
    assert!(snippet(&format!(
        "{base}fn g() -> i32 {{\n with (may_fail()) {{ v ->\n  return v\n }} else {{ e ->\n  return 0\n }}\n}}\n"
    ))
    .is_empty());
}

#[test]
fn try_needs_a_fallible_enclosing_function() {
    // spec §9.5
    let base = "fn may_fail() -> !i32 {\n return 1\n}\n";
    assert!(snippet(&format!("{base}fn g() -> i32 {{\n return try may_fail()\n}}\n"))
        .contains(&dz_diag::codes::TRY_OUTSIDE_FALLIBLE));
    assert!(snippet(&format!("{base}fn g() -> !i32 {{\n return try may_fail()\n}}\n")).is_empty());
}

#[test]
fn returning_a_plain_value_from_a_fallible_function_is_the_success_case() {
    // spec §9.1 — there is no wrapping step.
    assert!(snippet("fn f() -> !i32 {\n return 1\n}\n").is_empty());
    assert!(snippet("fn f() -> ?i32 {\n return none\n}\n").is_empty());
    assert!(snippet("fn f() -> !i32 {\n return error(\"bad\")\n}\n").is_empty());
    // `none` needs somewhere that wants a `?T`.
    assert!(snippet("fn f() {\n let x = none\n}\n")
        .contains(&dz_diag::codes::NO_CONTEXT_FOR_LITERAL));
}

#[test]
fn default_arguments_may_be_omitted() {
    // spec §7.2 — this only works because named calls go through the signature, not
    // through a bare function type, which does not record defaults.
    let base = "fn spawn(a: i32, hp: i32 = 100) -> i32 {\n return a + hp\n}\n";
    assert!(snippet(&format!("{base}fn g() -> i32 {{\n return spawn(1)\n}}\n")).is_empty());
    assert!(snippet(&format!("{base}fn g() -> i32 {{\n return spawn(1, 2)\n}}\n")).is_empty());
    assert!(snippet(&format!("{base}fn g() -> i32 {{\n return spawn(1, 2, 3)\n}}\n"))
        .contains(&dz_diag::codes::WRONG_ARG_COUNT));
}

#[test]
fn a_reference_cannot_be_stored_in_a_field() {
    // spec §5.2, §18.3 — the rule that removes the need for lifetimes.
    assert!(snippet("struct S {\n r: &i32\n}\n").contains(&dz_diag::codes::REF_IN_FIELD));
    assert!(snippet("struct S {\n r: ^i32\n}\n").is_empty());
}

#[test]
fn field_and_method_visibility_is_enforced_through_a_value() {
    // The `.` side of spec §4, which M2 could not reach without types.
    let base = "struct S {\n hidden: i32\n pub shown: i32\n}\n";
    assert!(snippet(&format!("{base}fn g(s: S) -> i32 {{\n return s.hidden\n}}\n"))
        .contains(&dz_diag::codes::PRIVATE_FIELD));
    assert!(snippet(&format!("{base}fn g(s: S) -> i32 {{\n return s.shown\n}}\n")).is_empty());
    // ...but the struct's own methods see everything.
    assert!(snippet("struct S {\n hidden: i32\n pub fn g(&self) -> i32 {\n  return hidden\n }\n}\n")
        .is_empty());
}

#[test]
fn dot_reaches_through_pointers_and_references() {
    // spec §5.2 — there is no `->`.
    assert!(snippet("struct S {\n pub n: i32\n}\nfn f(p: ^S, r: &S) -> i32 {\n return p.n + r.n\n}\n")
        .is_empty());
}

#[test]
fn generic_items_are_skipped_rather_than_misreported() {
    // Generics are M6. Until then a generic body must not produce spurious errors.
    // `Ord` is declared here rather than assumed: the operator traits of spec §11.2 are
    // stdlib, and there is no stdlib until M12.
    let codes = snippet(
        "trait Ord {\n fn cmp(&self, other: &Self) -> i32\n}\n\n\
         struct Buffer<T> {\n m_data: ^T\n m_len: u32\n \
         pub fn length(&self) -> u32 {\n  return m_len\n }\n}\n\n\
         fn f<T: Ord>(a: T, b: T) -> T {\n if a > b {\n  return a\n }\n return b\n}\n",
    );
    assert!(codes.is_empty(), "generic code should be skipped, got {codes:?}");
}
