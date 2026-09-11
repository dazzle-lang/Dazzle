//! End-to-end tests over real workspaces in `tests/workspace/` (spec §3, §4).
//!
//! These run the whole M2 pipeline — manifest, glob, lex, parse, resolve — so they cover
//! the milestone criterion directly: a two-module workspace that resolves names, and the
//! same shapes of code rejected when they reach across a visibility boundary.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use dz_diag::Diagnostics;
use dz_project::ModuleId;
use dz_span::SourceMap;

struct Checked {
    rendered: String,
    codes: Vec<&'static str>,
    error_count: usize,
    module_count: usize,
    file_count: usize,
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
            Some(i) => format!("{}{}", &l[..l[..i].rfind(' ').map_or(0, |s| s + 1)], &l[i + MARKER.len()..]),
            None => l.to_owned(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Run manifest loading, parsing and resolution over one workspace.
fn check(entry: &Path) -> Checked {
    let mut map = SourceMap::new();
    let mut diags = Diagnostics::new();

    let Some(ws) = dz_project::load(entry, &mut map, &mut diags) else {
        diags.sort_by_position();
        let codes = diags.iter().filter_map(|d| d.code).collect();
        let error_count = diags.error_count();
        return Checked {
            rendered: dz_diag::render_to_string(&map, &diags),
            codes,
            error_count,
            module_count: 0,
            file_count: 0,
        };
    };

    let mut files: HashMap<ModuleId, Vec<dz_resolve::ParsedFile>> = HashMap::new();
    let mut file_count = 0;
    for module in ws.modules() {
        let mut parsed = Vec::new();
        for path in &module.sources {
            let bytes = module.read_source(path).expect("fixture source");
            let id = map.add_bytes(path, &bytes).expect("fixture source is UTF-8");
            let text = map.get(id).text().to_owned();
            let lexed = dz_lex::lex(id, &text);
            let p = dz_parse::parse(id, &text, &lexed.tokens);
            diags.extend(lexed.diagnostics);
            diags.extend(p.diagnostics);
            parsed.push(dz_resolve::ParsedFile { file: id, ast: p.ast });
            file_count += 1;
        }
        files.insert(module.id, parsed);
    }

    if !diags.has_errors() {
        diags.extend(dz_resolve::resolve(&ws, &files).diagnostics);
    }
    diags.sort_by_position();

    let rendered = redact(&dz_diag::render_to_string(&map, &diags));

    Checked {
        codes: diags.iter().filter_map(|d| d.code).collect(),
        error_count: diags.error_count(),
        module_count: ws.len(),
        file_count,
        rendered,
    }
}

// --- the milestone criterion ---------------------------------------------

#[test]
fn a_three_module_workspace_resolves_cleanly() {
    let r = check(&fixture("good"));
    assert_eq!(r.error_count, 0, "expected a clean workspace:\n{}", r.rendered);
    // Four, not three: the prelude is a real module in every workspace (spec §18.23).
    assert_eq!(r.module_count, 4, "game plus two libraries plus the prelude");
    assert_eq!(r.file_count, 6);
}

#[test]
fn a_module_is_one_namespace_across_its_files() {
    // engine/internal.dz names `Layer`, `Renderer` and `make_renderer` from render.dz
    // without importing anything (spec §3.1). If that broke, `good` would not be clean.
    let r = check(&fixture("good"));
    assert!(r.codes.is_empty(), "{}", r.rendered);
}

#[test]
fn private_access_across_modules_is_rejected() {
    let r = check(&fixture("private"));
    for expected in [
        dz_diag::codes::PRIVATE_ITEM,
        dz_diag::codes::UNKNOWN_MODULE,
        dz_diag::codes::DUPLICATE_DEFINITION,
        dz_diag::codes::UNRESOLVED_NAME,
        dz_diag::codes::NO_SUCH_MEMBER,
        dz_diag::codes::SHADOWED_BINDING,
        dz_diag::codes::UNUSED_IMPORT,
    ] {
        assert!(
            r.codes.contains(&expected),
            "expected {expected} in:\n{}",
            r.rendered
        );
    }
    // Two private-access violations: a non-`pub` function, and a `pub(module)` field
    // reached from outside its module.
    let private = r.codes.iter().filter(|c| **c == dz_diag::codes::PRIVATE_ITEM).count();
    assert_eq!(private, 2, "{}", r.rendered);
}

#[test]
fn circular_module_dependencies_are_rejected() {
    let r = check(&fixture("cycle/a"));
    assert!(
        r.codes.contains(&dz_diag::codes::MODULE_CYCLE),
        "expected a cycle error:\n{}",
        r.rendered
    );
}

#[test]
fn manifest_problems_are_reported_with_spans() {
    let r = check(&fixture("badmanifest"));
    for expected in [
        dz_diag::codes::MANIFEST_BAD_VALUE,
        dz_diag::codes::MANIFEST_MISSING_FIELD,
        dz_diag::codes::MANIFEST_MISSING,
    ] {
        assert!(r.codes.contains(&expected), "expected {expected}:\n{}", r.rendered);
    }
    // Every manifest diagnostic must point somewhere, not just name the file.
    assert!(r.rendered.contains("dazzle.toml:"), "{}", r.rendered);
}

// --- snapshots ------------------------------------------------------------

#[test]
fn diagnostics_snapshot() {
    for name in ["good", "private", "cycle/a", "badmanifest"] {
        let r = check(&fixture(name));
        let body = if r.rendered.is_empty() { "(none)\n".to_owned() } else { r.rendered };
        insta::assert_snapshot!(
            format!("workspace_{}", name.replace('/', "_")),
            format!("modules: {}, files: {}\n\n{}", r.module_count, r.file_count, body)
        );
    }
}

// --- targeted rules -------------------------------------------------------

/// Resolve a single-file, single-module workspace built on the fly.
fn check_snippet(source: &str) -> Vec<&'static str> {
    let dir = std::env::temp_dir().join(format!("dz_resolve_{}", fastish_hash(source)));
    let _ = std::fs::create_dir_all(dir.join("src"));
    std::fs::write(
        dir.join("dazzle.toml"),
        "[module]\nname = \"t\"\nsources = [\"src/**/*.dz\"]\n",
    )
    .unwrap();
    std::fs::write(dir.join("src/a.dz"), source).unwrap();
    let r = check(&dir);
    let _ = std::fs::remove_dir_all(&dir);
    r.codes
}

fn fastish_hash(s: &str) -> u64 {
    let mut h = 0xcbf29ce484222325u64;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

#[test]
fn builtin_types_and_functions_resolve() {
    let codes = check_snippet(
        "fn f(a: i32, b: f32, c: str, d: bool) -> usize {\n    \
             println(\"{}\", a)\n    \
             return sizeof(i32)\n\
         }\n",
    );
    assert!(codes.is_empty(), "{codes:?}");
}

#[test]
fn a_local_shadows_nothing_it_should_not() {
    // Same scope: rejected. Inner scope: allowed.
    assert!(check_snippet("fn f() {\n let a = 1\n let a = 2\n}\n")
        .contains(&dz_diag::codes::SHADOWED_BINDING));
    assert!(check_snippet("fn f() {\n let a = 1\n {\n let a = 2\n }\n}\n").is_empty());
}

#[test]
fn method_bodies_see_their_struct_fields_unqualified() {
    // spec §7.3 — `m_count` resolves without `self.`
    let codes = check_snippet(
        "struct S {\n    m_count: u32\n    pub fn bump(&mut self) {\n        \
         m_count += 1\n    }\n}\n",
    );
    assert!(codes.is_empty(), "{codes:?}");
}

#[test]
fn an_or_clause_binds_the_error_only_when_it_is_named() {
    // spec §9.4 — there is no implicit `it`.
    let codes = check_snippet(
        "fn f(x: i32?) {\n    let v = x or { return }\n}\n",
    );
    assert!(codes.is_empty(), "{codes:?}");

    let named = check_snippet(
        "fn g() !-> i32 {\n    return 1\n}\n\
         fn f() {\n    let v = g() or { e ->\n        println(\"{}\", e.message)\n        return\n    }\n}\n",
    );
    assert!(named.is_empty(), "{named:?}");

    // `it` is not in scope: the binding must be written out.
    let implicit = check_snippet(
        "fn f(x: i32?) {\n    let v = x or {\n        println(\"{}\", it)\n        return\n    }\n}\n",
    );
    assert!(implicit.contains(&dz_diag::codes::UNRESOLVED_NAME), "{implicit:?}");
}

#[test]
fn declaration_order_within_a_module_does_not_matter() {
    let codes = check_snippet(
        "fn first() -> i32 {\n    return second()\n}\n\nfn second() -> i32 {\n    return 1\n}\n",
    );
    assert!(codes.is_empty(), "{codes:?}");
}

#[test]
fn a_let_initialiser_cannot_see_its_own_binding() {
    // `let x = x` must refer to an outer `x`, and there is none here.
    let codes = check_snippet("fn f() {\n    let x = x\n}\n");
    assert!(codes.contains(&dz_diag::codes::UNRESOLVED_NAME), "{codes:?}");
}
