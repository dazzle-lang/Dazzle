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
    /// The HIR the checker built, dumped (spec §17.1).
    hir: String,
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/workspace").join(name)
}

/// Snapshots must not depend on where the repository is checked out. Diagnostics name
/// files by absolute path, so normalise separators first (the rendered text is native,
/// with backslashes on Windows) and then drop everything above the fixture root.
fn redact(rendered: &str) -> String {
    const MARKER: &str = "tests/workspace/";
    rendered
        .replace('\\', "/")
        .lines()
        .map(|l| match l.find(MARKER) {
            Some(i) => format!(
                "{}{}",
                &l[..l[..i].rfind(' ').map_or(0, |s| s + 1)],
                &l[i + MARKER.len()..]
            ),
            None => l.to_owned(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Run the whole pipeline: manifest, lex, parse, resolve, type-check.
fn check(entry: &Path) -> Checked {
    let mut map = SourceMap::new();
    let mut diags = Diagnostics::new();

    let Some(ws) = dz_project::load(entry, &mut map, &mut diags) else {
        return Checked {
            rendered: "failed to load workspace".into(),
            codes: vec![],
            error_count: 1,
            hir: String::new(),
        };
    };

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
    assert!(!diags.has_errors(), "fixture should lex and parse cleanly");

    let resolution = dz_resolve::resolve(&ws, &files);
    let resolve_failed = resolution.diagnostics.has_errors();
    diags.extend(resolution.diagnostics.clone());
    let mut hir = String::new();
    if !resolve_failed {
        let checked = dz_check::check(&ws, &resolution, &files, &map);
        hir = dz_hir::dump::dump(&checked.hir, &dz_check::Names { res: &resolution });
        diags.extend(checked.diagnostics);
    }
    diags.sort_by_position();

    let rendered = redact(&dz_diag::render_to_string(&map, &diags));

    Checked {
        codes: diags.iter().filter_map(|d| d.code).collect(),
        error_count: diags.error_count(),
        rendered,
        hir,
    }
}

/// Type-check a snippet as a one-file module.
fn snippet(source: &str) -> Vec<&'static str> {
    let dir = std::env::temp_dir().join(format!("dz_check_{:x}", hash(source)));
    let _ = std::fs::create_dir_all(dir.join("src"));
    std::fs::write(
        dir.join("dazzle.toml"),
        "[module]\nname = \"t\"\nsources = [\"src/**/*.dz\"]\n\n[dependencies]\nstd = { open = true }\n",
    )
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
        // M6 — generics and traits (spec §7.6, §8).
        NOT_A_TRAIT,
        UNSATISFIED_BOUND,
        MISSING_TRAIT_MEMBER,
        TRAIT_MEMBER_MISMATCH,
        WRONG_TYPE_ARG_COUNT,
        // M6 — format strings (spec §12.9).
        BAD_FORMAT_STRING,
        NOT_FORMATTABLE,
        // r12 — `unsafe`, and one trait at several arguments (spec §5.2, §18.27).
        UNSAFE_OPERATION,
        IF_LET_FALLIBLE,
        NOT_FALLIBLE,
        DUPLICATE_METHOD,
        AMBIGUOUS_CALL,
        // r13 — implementations (spec §6.1, §7.6).
        ORPHAN_IMPL,
        OVERLAPPING_IMPL,
        // Struct inheritance (spec §7.3, 18.32).
        BAD_BASE,
        SHADOWS_BASE_MEMBER,
        MISSING_BASE_INIT,
        BAD_OVERRIDE,
        ABSTRACT_TYPE,
        // M14 — tests as a language feature (spec §19).
        BAD_TEST,
        // M8 — `switch` on constants (spec §10.3).
        BAD_CASE_LABEL,
        DUPLICATE_CASE,
        // r38 — a bare variant carries the struct it names (spec §7.5, 18.45).
        BAD_VARIANT,
        RETURN_IN_BASE_INIT,
        // r39 — statics (spec §7.7).
        STATIC_INITIALISER,
        // r55 — generic methods (spec §8.1).
        GENERIC_METHOD_NOT_ALLOWED,
        // r63 — a union's methods (spec §7.5, 18.78).
        RESERVED_UNION_METHOD,
        // r62 — arguments passed by name (spec §7.2).
        BAD_NAMED_ARG,
    ] {
        assert!(r.codes.contains(&expected), "expected {expected} in:\n{}", r.rendered);
    }
}

/// Spec §5.2: one missing `unsafe` is one mistake, however many raw-memory operations
/// sit inside the expression it should have wrapped.
#[test]
fn one_missing_unsafe_is_one_diagnostic() {
    let codes = snippet("fn f(p: ^mut i32) {\n destroy(p[0])\n}\n");
    let n = codes.iter().filter(|c| **c == dz_diag::codes::UNSAFE_OPERATION).count();
    assert_eq!(n, 1, "{codes:?}");
    assert!(snippet("fn f(p: ^mut i32) {\n unsafe { destroy(p[0]) }\n}\n").is_empty());
}

#[test]
fn diagnostics_snapshot() {
    for name in ["typed", "typeerrors"] {
        let r = check(&fixture(name));
        let body = if r.rendered.is_empty() { "(none)\n".to_owned() } else { r.rendered };
        insta::assert_snapshot!(format!("check_{name}"), body);
    }
}

/// The HIR is the input to flow analysis and, from M5 on, to codegen; if it is wrong,
/// every later pass is wrong in a way that only surfaces as bad C++. Snapshotting the whole
/// tree for the `typed/` fixture pins its shape and every type on it.
#[test]
fn hir_snapshot() {
    let r = check(&fixture("typed"));
    assert_eq!(r.error_count, 0, "fixture should check cleanly:\n{}", r.rendered);
    assert!(!r.hir.is_empty(), "the checker built no bodies");
    insta::assert_snapshot!("hir_typed", r.hir);
}

/// Every expression node must carry a real type. `{integer}` and `{float}` are states the
/// checker passes through while a literal waits for context (spec §2.5); a consumer that
/// saw one would have to re-run literal defaulting itself, which is exactly the duplicated
/// inference the HIR exists to prevent.
#[test]
fn the_hir_holds_no_untyped_literals() {
    let r = check(&fixture("typed"));
    for (i, line) in r.hir.lines().enumerate() {
        assert!(
            !line.contains("{integer}") && !line.contains("{float}"),
            "line {}: untyped literal survived into the HIR: {line}",
            i + 1
        );
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
    assert!(
        snippet("fn f() {\n let b: u8 = 300\n}\n").contains(&dz_diag::codes::LITERAL_OUT_OF_RANGE)
    );
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
    assert!(snippet("struct S {\n n: i32\n pub fn bad(&self) {\n  n = 1\n }\n}\n")
        .contains(&dz_diag::codes::NOT_MUTABLE));
    assert!(snippet("struct S {\n n: i32\n pub fn ok(&mut self) {\n  n = 1\n }\n}\n").is_empty());
}

#[test]
fn switch_must_cover_every_variant() {
    // spec §10.3
    let missing = "union U {\n A {}\n B {}\n}\nfn f(u: U) -> i32 {\n switch u {\n  case A {\n   return 1\n  }\n }\n return 0\n}\n";
    assert!(snippet(missing).contains(&dz_diag::codes::NON_EXHAUSTIVE_SWITCH));
    let covered = "union U {\n A {}\n B {}\n}\nfn f(u: U) -> i32 {\n switch u {\n  case A {\n   return 1\n  }\n  case B {\n   return 2\n  }\n }\n return 0\n}\n";
    assert!(snippet(covered).is_empty());
}

#[test]
fn switch_on_a_scalar_matches_constants() {
    // spec §10.3 — no exhaustiveness on an integer, but a label must be known while
    // compiling, and each value may be matched once.
    use dz_diag::codes::{BAD_CASE_LABEL, DUPLICATE_CASE, TYPE_MISMATCH};
    let ok = "const N = 4\nfn f(n: i32, b: u8, s: str) {\n switch n {\n  case 1, N, N + 1 {}\n }\n switch b {\n  case 255 {}\n }\n switch s {\n  case \"a\" {}\n  default {}\n }\n}\n";
    assert!(snippet(ok).is_empty(), "{:?}", snippet(ok));
    let run_time = "fn f(n: i32, m: i32) {\n switch n {\n  case m {}\n }\n}\n";
    assert_eq!(snippet(run_time), vec![BAD_CASE_LABEL]);
    let twice = "const N = 4\nfn f(n: i32) {\n switch n {\n  case 4 {}\n  case 2 + 2 {}\n  case N {}\n }\n}\n";
    assert_eq!(snippet(twice), vec![DUPLICATE_CASE, DUPLICATE_CASE]);
    let binding = "fn f(n: i32) {\n switch n {\n  case X(x) {}\n }\n}\n";
    assert_eq!(snippet(binding), vec![BAD_CASE_LABEL]);
    let float = "fn f(x: f32) {\n switch x {\n  default {}\n }\n}\n";
    assert_eq!(snippet(float), vec![TYPE_MISMATCH]);
    let out_of_range = "fn f(n: u8) {\n switch n {\n  case 300 {}\n }\n}\n";
    assert!(!snippet(out_of_range).is_empty());
    // An enum stays exhaustive, and names its variants rather than values.
    let enum_value =
        "enum E {\n A\n B\n}\nfn f(e: E) {\n switch e {\n  case 0 {}\n  default {}\n }\n}\n";
    assert_eq!(snippet(enum_value), vec![BAD_CASE_LABEL]);
    let enum_twice =
        "enum E {\n A\n B\n}\nfn f(e: E) {\n switch e {\n  case A, B {}\n  case A {}\n }\n}\n";
    assert_eq!(snippet(enum_twice), vec![DUPLICATE_CASE]);
}

#[test]
fn a_bare_variant_carries_the_struct_it_names() {
    // spec §7.5, 18.45 — `Idle` carries `struct Idle`, methods and defaults included; a
    // variant with no payload is `Off {}`, and a bare name with no struct behind it is an
    // error rather than an empty variant.
    use dz_diag::codes::{BAD_VARIANT, MISSING_FIELD};
    let ok = "struct Idle {\n pub t: i32 = 1\n pub fn next(&self) -> i32 {\n  return t + 1\n }\n}\n\
              union S {\n Idle\n Off {}\n}\n\
              fn f() -> i32 {\n let a: S = .Idle{ t: 2 }\n let b = S::Idle\n let c: S = S::Off\n \
              switch c {\n  case Off {}\n  default {}\n }\n switch b {\n  case Idle(i) { return i.t }\n  default {}\n }\n \
              switch a {\n  case Idle(&i) { return i.next() }\n  case Off { return 0 }\n }\n}\n";
    assert!(snippet(ok).is_empty(), "{:?}", snippet(ok));
    let missing = "union S {\n Idle\n}\n";
    assert_eq!(snippet(missing), vec![BAD_VARIANT]);
    let not_a_struct = "enum Idle {\n A\n}\nunion S {\n Idle\n}\n";
    assert_eq!(snippet(not_a_struct), vec![BAD_VARIANT]);
    let generic = "struct Idle<T> {\n pub v: T\n}\nunion S {\n Idle\n}\n";
    assert_eq!(snippet(generic), vec![BAD_VARIANT]);
    // Built as the struct is, so a field without a default must be given.
    let no_default =
        "struct Idle {\n pub t: i32\n}\nunion S {\n Idle\n}\nfn f() {\n let a: S = .Idle{}\n}\n";
    assert_eq!(snippet(no_default), vec![MISSING_FIELD]);
}

#[test]
fn a_generic_union_is_read_through_its_arguments() {
    // spec §7.5, §8.4 — `case One(o)` on a `Choice<i32>` makes `o.value` an `i32`.
    let base = "union Choice<T> {\n One { value: T }\n Neither {}\n}\n";
    let ok = format!(
        "{base}fn f() -> i32 {{\n let c = Choice<i32>::One{{ value: 1 }}\n switch c {{\n  case One(o) {{ return o.value }}\n  case Neither {{ return 0 }}\n }}\n}}\n"
    );
    assert!(snippet(&ok).is_empty(), "{:?}", snippet(&ok));
    let wrong = format!(
        "{base}fn f() -> f32 {{\n let c = Choice<i32>::One{{ value: 1 }}\n switch c {{\n  case One(o) {{ return o.value }}\n  case Neither {{ return 0 }}\n }}\n}}\n"
    );
    assert_eq!(snippet(&wrong), vec![dz_diag::codes::TYPE_MISMATCH]);
    let payload = format!("{base}fn f() {{\n let c = Choice<i32>::One{{ value: 1.5 }}\n}}\n");
    assert_eq!(snippet(&payload), vec![dz_diag::codes::TYPE_MISMATCH]);
    // Nothing to take the arguments from.
    let bare = format!("{base}fn f() {{\n let c = Choice::Neither\n}}\n");
    assert_eq!(snippet(&bare), vec![dz_diag::codes::WRONG_TYPE_ARG_COUNT]);
}

#[test]
fn a_fallible_call_must_be_consumed_in_its_statement() {
    // spec §9.7 — there is no `T!` type, so there is nowhere else for it to go.
    let base = "fn may_fail() !-> i32 {\n return 1\n}\n";
    assert!(snippet(&format!("{base}fn g() {{\n may_fail()\n}}\n"))
        .contains(&dz_diag::codes::UNUSED_RESULT));
    // A binding is no longer an escape hatch: the value would outlive its statement.
    assert!(snippet(&format!("{base}fn g() {{\n let _ = may_fail()\n}}\n"))
        .contains(&dz_diag::codes::UNUSED_RESULT));
    // Each sanctioned way to consume it is accepted.
    assert!(snippet(&format!("{base}fn g() {{\n may_fail() or ignore\n}}\n")).is_empty());
    assert!(snippet(&format!(
        "{base}fn g() !-> i32 {{\n let v = may_fail() or return\n return v\n}}\n"
    ))
    .is_empty());
    assert!(snippet(&format!("{base}fn g() -> i32 {{\n return may_fail() or 0\n}}\n")).is_empty());
    assert!(snippet(&format!(
        "{base}fn g() -> i32 {{\n let v = may_fail() or {{ e ->\n  return 0\n }}\n return v\n}}\n"
    ))
    .is_empty());
}

#[test]
fn or_return_needs_a_signature_that_can_carry_the_failure() {
    // spec §9.5 — bare `or return` is reserved for propagation.
    let base = "fn may_fail() !-> i32 {\n return 1\n}\n";
    assert!(snippet(&format!("{base}fn g() -> i32 {{\n return may_fail() or return\n}}\n"))
        .contains(&dz_diag::codes::TRY_OUTSIDE_FALLIBLE));
    assert!(
        snippet(&format!("{base}fn g() !-> i32 {{\n return may_fail() or return\n}}\n")).is_empty()
    );
    // A `T?` propagates into a `-> U?` or a `-> void`, which have somewhere to go.
    let opt = "fn maybe() -> i32? {\n return none\n}\n";
    assert!(snippet(&format!("{opt}fn g() -> i32? {{\n return maybe() or return\n}}\n")).is_empty());
    assert!(snippet(&format!("{opt}fn g() {{\n let v = maybe() or return\n}}\n")).is_empty());
}

#[test]
fn a_pointer_becomes_a_reference_only_through_an_or_clause() {
    // spec §5.2 — this is the one guarantee `&T` carries that `^T` does not.
    let base = "fn takes(x: &i32) {\n}\nfn ptr() -> ^i32 {\n return null\n}\n";
    assert!(snippet(&format!("{base}fn g() {{\n takes(ptr())\n}}\n"))
        .contains(&dz_diag::codes::POINTER_NEEDS_CHECK));
    assert!(
        snippet(&format!("{base}fn g() {{\n let p = ptr() or return\n takes(p)\n}}\n")).is_empty()
    );
    assert!(snippet(&format!("{base}fn g() {{\n let p = ptr() or unreachable\n takes(p)\n}}\n"))
        .is_empty());
    // The reverse direction stays implicit (spec §5.2, §18.16).
    assert!(snippet("fn g(r: &mut i32) {\n let p: ^i32 = r\n}\n").is_empty());
}

#[test]
fn a_dot_variant_needs_an_enum_or_union_to_come_from() {
    // `.Idle` takes its variant from the expected type; where that is a number there is
    // nothing to infer from, and saying nothing let the mistake through in silence.
    let codes = snippet("fn f(n: u32) {\n}\nfn g() {\n f(.Nope)\n}\n");
    assert_eq!(codes, vec![dz_diag::codes::NO_SUCH_FIELD], "{codes:?}");
    assert!(snippet("enum E {\n A\n B\n}\nfn f(e: E) {\n}\nfn g() {\n f(.A)\n}\n").is_empty());
}

#[test]
fn a_string_literal_is_a_cstr_where_one_is_wanted() {
    // spec §5.3, §18.39 — a literal is zero-terminated static storage already.
    assert!(snippet("fn f(s: cstr) {\n}\nfn g() {\n f(\"x\")\n}\n").is_empty());
    assert!(snippet("fn g() {\n let c: cstr = \"x\"\n let d = \"y\" as cstr\n}\n").is_empty());
    assert!(snippet("fn g(c: cstr) -> str {\n return c as str\n}\n").is_empty());
    // A runtime `str` has no terminating zero, so it needs a copy — not yet.
    assert!(snippet("fn g(s: str) -> cstr {\n return s as cstr\n}\n")
        .contains(&dz_diag::codes::INVALID_CAST));
    // Without a `cstr` wanted, a literal is a `str` as before.
    assert!(snippet("fn f(s: cstr) {\n}\nfn g() {\n let s = \"x\"\n f(s)\n}\n")
        .contains(&dz_diag::codes::TYPE_MISMATCH));
    // `cstr("x")` is one mistake at the type name, not a silent error that surfaces later as
    // "`{unknown}` cannot be formatted".
    let codes = snippet("fn g() {\n let c = cstr(\"x\")\n println!(\"{}\", c)\n}\n");
    assert_eq!(codes, vec![dz_diag::codes::NOT_CALLABLE], "{codes:?}");
}

#[test]
fn a_pointer_is_as_writable_as_it_says() {
    // spec §5.2, §18.37 — `^T` and `^mut T` stand to each other as `&T` and `&mut T` do.
    assert!(snippet("fn g(r: &i32) {\n let p: ^i32 = r\n}\n").is_empty());
    assert!(snippet("fn g(r: &mut i32) {\n let p: ^mut i32 = r\n}\n").is_empty());
    assert!(snippet("fn g(q: ^mut i32) {\n let p: ^i32 = q\n}\n").is_empty());
    assert!(snippet("fn g(r: &i32) {\n let p: ^mut i32 = r\n}\n")
        .contains(&dz_diag::codes::TYPE_MISMATCH));
    assert!(snippet("fn g(q: ^i32) {\n let p: ^mut i32 = q\n}\n")
        .contains(&dz_diag::codes::TYPE_MISMATCH));
    // Writing needs the `mut`; reading does not.
    assert!(snippet("fn g(p: ^i32) -> i32 {\n unsafe { return p^ }\n}\n").is_empty());
    assert!(snippet("fn g(p: ^mut i32) {\n unsafe { p^ = 1 }\n}\n").is_empty());
    assert!(
        snippet("fn g(p: ^i32) {\n unsafe { p^ = 1 }\n}\n").contains(&dz_diag::codes::NOT_MUTABLE)
    );
    // Through a writable pointer the pointee cannot widen: a `^mut ^i32` made from a
    // `^mut ^mut i32` could store a read-only pointer where a writable one is expected.
    assert!(snippet("fn g(q: ^^mut i32) {\n let p: ^^i32 = q\n}\n").is_empty());
    assert!(snippet("fn g(q: ^mut ^mut i32) {\n let p: ^mut ^i32 = q\n}\n")
        .contains(&dz_diag::codes::TYPE_MISMATCH));
}

#[test]
fn an_or_clause_must_produce_the_success_type_or_diverge() {
    // spec §9.4 — the single rule governing every clause form.
    let opt = "fn maybe() -> i32? {\n return none\n}\n";
    assert!(snippet(&format!("{opt}fn g() -> i32 {{\n return maybe() or \"no\"\n}}\n"))
        .contains(&dz_diag::codes::OR_CLAUSE_TYPE));
    assert!(snippet(&format!("{opt}fn g() -> i32 {{\n return maybe() or 0\n}}\n")).is_empty());
    // Diverging satisfies the rule without producing anything.
    assert!(snippet(&format!("{opt}fn g() -> i32 {{\n return maybe() or panic!(\"x\")\n}}\n"))
        .is_empty());
    // Only a fallible call carries an `Error`, so a `T?` has nothing to bind.
    assert!(snippet(&format!(
        "{opt}fn g() -> i32 {{\n let v = maybe() or {{ e ->\n  return 0\n }}\n return v\n}}\n"
    ))
    .contains(&dz_diag::codes::OR_CLAUSE_TYPE));
    // `or` on something that cannot fail is meaningless.
    assert!(snippet("fn g(n: i32) -> i32 {\n return n or 0\n}\n")
        .contains(&dz_diag::codes::NOT_FALLIBLE));
}

#[test]
fn a_fallback_that_can_itself_fail_gets_a_clause_of_its_own() {
    // spec §9.4 — "try a, else try b, else c" is a real thing to want, and the block form
    // is how it is written. Flat chaining cannot mean it: an `or` always produces the
    // success type, so `a() or b()` can no longer fail and has nothing left to catch.
    let base = "fn a() !-> i32 {\n return 1\n}\nfn b() !-> i32 {\n return 2\n}\n";
    assert!(
        snippet(&format!("{base}fn g() -> i32 {{\n return a() or {{ b() or 0 }}\n}}\n")).is_empty()
    );
    assert!(snippet(&format!("{base}fn g() -> i32 {{\n return a() or {{ b() or {{ 0 }} }}\n}}\n"))
        .is_empty());
    // Without a clause of its own, the fallback's failure would be discarded.
    let flat = snippet(&format!("{base}fn g() -> i32 {{\n return (a() or b()) or 0\n}}\n"));
    assert!(flat.contains(&dz_diag::codes::UNUSED_RESULT), "{flat:?}");
    assert!(flat.contains(&dz_diag::codes::NOT_FALLIBLE), "{flat:?}");
}

#[test]
fn returning_a_plain_value_from_a_fallible_function_is_the_success_case() {
    // spec §9.1 — there is no wrapping step.
    assert!(snippet("fn f() !-> i32 {\n return 1\n}\n").is_empty());
    assert!(snippet("fn f() -> i32? {\n return none\n}\n").is_empty());
    assert!(snippet("fn f() !-> i32 {\n return error(\"bad\")\n}\n").is_empty());
    // `error(...)` needs a signature that can carry it (spec §9.1).
    assert!(snippet("fn f() -> i32 {\n return error(\"bad\")\n}\n")
        .contains(&dz_diag::codes::TRY_OUTSIDE_FALLIBLE));
    // `none` needs somewhere that wants a `?T`.
    assert!(
        snippet("fn f() {\n let x = none\n}\n").contains(&dz_diag::codes::NO_CONTEXT_FOR_LITERAL)
    );
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
    assert!(snippet(
        "struct S {\n hidden: i32\n pub fn g(&self) -> i32 {\n  return hidden\n }\n}\n"
    )
    .is_empty());
}

#[test]
fn dot_reaches_through_pointers_and_references() {
    // spec §5.2 — there is no `->`. Through a `^T` it is a raw read, so it needs `unsafe`;
    // through a reference it does not.
    let s = "struct S {\n pub n: i32\n}\n";
    assert!(snippet(&format!(
        "{s}fn f(p: ^S, r: &S) -> i32 {{\n unsafe {{ return p.n + r.n }}\n}}\n"
    ))
    .is_empty());
    assert!(snippet(&format!("{s}fn f(r: &S) -> i32 {{\n return r.n\n}}\n")).is_empty());
    assert!(snippet(&format!("{s}fn f(p: ^S) -> i32 {{\n return p.n\n}}\n"))
        .contains(&dz_diag::codes::UNSAFE_OPERATION));
}

/// A generic body is checked **once**, against its bounds (spec §8.1, §8.6). `Ord` here is
/// the prelude's, in scope with no import (§18.23), which is what makes `a > b` legal.
#[test]
fn a_generic_body_is_checked_against_its_bounds() {
    let codes = snippet(
        "struct Buffer<T> {\n m_data: ^T\n m_len: u32\n \
         pub fn length(&self) -> u32 {\n  return m_len\n }\n}\n\n\
         fn f<T: Ord>(a: T, b: T) -> T {\n if a > b {\n  return a\n }\n return b\n}\n",
    );
    assert!(codes.is_empty(), "valid generic code was rejected: {codes:?}");
}

/// ...and only against its bounds: the same body without the bound must be rejected, at the
/// line inside the generic rather than at any instantiation.
#[test]
fn a_generic_body_may_not_exceed_its_bounds() {
    let codes = snippet("fn f<T>(a: T, b: T) -> T {\n if a > b {\n  return a\n }\n return b\n}\n");
    assert!(
        codes.contains(&dz_diag::codes::UNSATISFIED_BOUND),
        "an unbounded `T` has no `>`, got {codes:?}"
    );
}

/// Operators are traits (spec §11.2). Comparing a struct that implements neither `Eq` nor
/// `Ord` used to reach C++ and fail there, which invariant 7 forbids.
#[test]
fn comparing_a_struct_needs_eq() {
    let without =
        snippet("struct P {\n pub n: i32\n}\nfn f(a: P, b: P) -> bool {\n return a == b\n}\n");
    assert!(
        without.contains(&dz_diag::codes::UNSATISFIED_BOUND),
        "a struct with no `Eq` must not be comparable, got {without:?}"
    );
    let with = snippet(
        "struct P {\n pub n: i32\n}\n\
         impl Eq for P {\n fn eq(&self, rhs: &P) -> bool {\n  return n == rhs.n\n }\n}\n\
         fn f(a: P, b: P) -> bool {\n return a == b\n}\n",
    );
    assert!(with.is_empty(), "a struct implementing `Eq` is comparable, got {with:?}");
}

/// Spec §7.6: an implementation may be written where the trait is declared, or where the
/// type is — so a module can give a type it did not write behaviour of its own, `f32`
/// included — and nowhere else.
#[test]
fn an_impl_belongs_with_its_trait_or_its_type() {
    let own_trait = snippet(
        "trait Named {\n fn name(&self) -> str\n}\n\
         impl Named for f32 {\n fn name(&self) -> str {\n  return \"f32\"\n }\n}\n\
         fn f(x: f32) -> str {\n return x.name()\n}\n",
    );
    assert!(own_trait.is_empty(), "a module's own trait for `f32`, got {own_trait:?}");
    let neither = snippet(
        "impl Add for i32 {\n type Output = i32\n fn add(&self, rhs: i32) -> i32 {\n  return 0\n }\n}\n",
    );
    assert_eq!(neither, vec![dz_diag::codes::ORPHAN_IMPL], "one diagnostic, for the orphan");
}

/// Spec §7.2, T-0036: a call reaches the constructor its arguments fit — by count, defaults
/// counted, then by type, exact types settling a tie — and is ambiguous when nothing settles
/// it. It used to take the first constructor whatever the arguments said.
#[test]
fn constructors_are_resolved_by_argument() {
    let decl = "struct R {\n pub w: f32\n R(w: f32, h: f32 = 1.0) {\n  self.w = w * h\n }\n \
                R(n: i32) {\n  self.w = n as f32\n }\n R(n: i64, m: i64) {\n  self.w = 0.0\n }\n}\n";
    for call in ["R(2.0)", "R(2.0, 3.0)", "R(2)", "R(2 as i64, 3)"] {
        let codes = snippet(&format!("{decl}fn f() -> f32 {{\n return {call}.w\n}}\n"));
        assert!(codes.is_empty(), "`{call}` reaches one constructor, got {codes:?}");
    }
    let ambiguous = snippet(
        "struct S {\n pub a: i32\n S(a: i32, b: i32 = 0) {\n  self.a = a + b\n }\n \
         S(a: i32) {\n  self.a = a\n }\n}\nfn f() -> i32 {\n return S(1).a\n}\n",
    );
    assert_eq!(ambiguous, vec![dz_diag::codes::AMBIGUOUS_CALL], "both take one `i32`");
}

/// Spec §7.6, T-0080: a method call reaches through a pointer or reference to what it points
/// at (§5.2), so an implementation for the pointer type itself is refused — it used to pass
/// the checker and reach C++ as a call on the pointee. Wrapping the pointer is the way.
#[test]
fn an_impl_for_a_pointer_type_is_refused() {
    for self_ty in ["^P", "^mut P", "&P"] {
        let codes = snippet(&format!(
            "trait Show {{\n fn show(&self) -> i32\n}}\nstruct P {{\n}}\n\
             impl Show for {self_ty} {{\n fn show(&self) -> i32 {{\n  return 1\n }}\n}}\n"
        ));
        assert_eq!(codes, vec![dz_diag::codes::ORPHAN_IMPL], "`impl Show for {self_ty}`");
    }
    let generic = snippet(
        "trait Show {\n fn show(&self) -> i32\n}\n\
         impl<T> Show for ^T {\n fn show(&self) -> i32 {\n  return 1\n }\n}\n",
    );
    assert_eq!(generic, vec![dz_diag::codes::ORPHAN_IMPL], "`impl<T> Show for ^T`");
    let wrapped = snippet(
        "trait Show {\n fn show(&self) -> i32\n}\nstruct P {\n}\n\
         struct ById {\n pub p: ^P\n}\n\
         impl Show for ById {\n fn show(&self) -> i32 {\n  return 1\n }\n}\n\
         fn twice<T: Show>(x: T) -> i32 {\n return x.show() * 2\n}\n\
         fn f(p: ^P) -> i32 {\n return twice(ById{ p: p })\n}\n",
    );
    assert!(wrapped.is_empty(), "a struct wrapping the pointer, got {wrapped:?}");
}
