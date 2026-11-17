//! A C++ namespace named bare by its dependency's `namespaces` (spec §15.2, T-0179):
//! `gui::draw` where the header's namespace is `gui`, and `lib::gui::draw` still. The
//! resolver and the checker must agree on it, so a program written that way checks clean
//! all the way through, struct literals included; and every way the name can be taken is
//! one error on the manifest's line.
//!
//! Each case is a small workspace written to a temporary folder around one header. Reading
//! a header needs libclang, which a machine may not have: then the case says so and passes,
//! as the other tests that import headers do.

use std::path::{Path, PathBuf};

use dz_diag::{codes, Severity};

const HEADER: &str = "\
#pragma once
namespace gui {
struct Style { int size; };
inline int draw(int x) { return x + 1; }
namespace detail {
inline int inner() { return 7; }
}
}
inline int global_fn() { return 3; }
";

/// A workspace whose dependency `lib` is `HEADER`, with `dep` as the dependency's line and
/// `source` as its one file.
fn workspace(tag: &str, dep: &str, source: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("dz_namespaces_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::create_dir_all(dir.join("vendor")).unwrap();
    std::fs::write(dir.join("vendor/lib.hpp"), HEADER).unwrap();
    std::fs::write(
        dir.join("dazzle.toml"),
        format!(
            "[module]\nname = \"app\"\nsources = [\"src/**/*.dz\"]\n\n[dependencies]\n{dep}\n\
             std = {{ open = true }}\n\n[cpp.libs.lib]\nroot = \"vendor\"\ninclude_dirs = [\".\"]\n\
             import = [\"lib.hpp\"]\nkeeps = []\n\n[build]\noutput = \"executable\"\n"
        ),
    )
    .unwrap();
    std::fs::write(dir.join("src/main.dz"), source).unwrap();
    dir
}

/// The analysis, or `None` when this machine cannot read a header at all.
fn analyse(dir: &Path) -> Option<dz_driver::Analysis> {
    let a = dz_driver::analyse(dir);
    if a.diags.iter().any(|d| d.code == Some(codes::CPP_UNAVAILABLE)) {
        eprintln!("skipping: libclang is not available");
        return None;
    }
    Some(a)
}

fn errors(a: &dz_driver::Analysis) -> Vec<&'static str> {
    let mut out: Vec<&'static str> =
        a.diags.iter().filter(|d| d.severity == Severity::Error).filter_map(|d| d.code).collect();
    out.sort();
    out
}

const USES: &str = "\
fn main() {
    let s = gui::Style{ size: 2 }
    let t: gui::Style = s
    println!(\"{} {} {}\", gui::draw(t.size), detail::inner(), lib::global_fn())
    println!(\"{}\", lib::gui::draw(1) + lib::gui::detail::inner())
}
";

#[test]
fn a_namespace_named_bare_resolves_and_type_checks() {
    let dir =
        workspace("ok", "lib = { cpp = \"lib\", namespaces = [\"gui\", \"gui::detail\"] }", USES);
    let Some(a) = analyse(&dir) else { return };
    let rendered = dz_diag::render_to_string(&a.map, &a.diags);
    assert!(!a.diags.has_errors(), "{rendered}");
    // Checking ran, so the paths resolution took through the namespaces were typed too:
    // a struct literal and an annotation start at the bare namespace.
    assert!(a.hir.is_some() && a.mono.is_some(), "{rendered}");
}

#[test]
fn without_namespaces_the_bare_name_is_unresolved() {
    let dir = workspace("absent", "lib = { cpp = \"lib\" }", USES);
    let Some(a) = analyse(&dir) else { return };
    let rendered = dz_diag::render_to_string(&a.map, &a.diags);
    assert!(errors(&a).iter().all(|c| *c == codes::UNRESOLVED_NAME), "{rendered}");
    assert!(a.diags.has_errors(), "{rendered}");
}

/// Each way the name can already be in use, and a namespace the header does not have: one
/// error each, on the manifest, and nothing in the code that uses it.
#[test]
fn a_name_already_taken_is_one_error_on_the_manifest() {
    let cases: &[(&str, &str, &str, &str)] = &[
        // Another dependency key.
        (
            "key",
            "lib = { cpp = \"lib\", namespaces = [\"gui\"] }\ngui = { cpp = \"lib\" }",
            "fn main() {\n    println!(\"{}\", gui::global_fn())\n}\n",
            codes::NAMESPACE_NAME_TAKEN,
        ),
        // A declaration of the module.
        (
            "decl",
            "lib = { cpp = \"lib\", namespaces = [\"gui\"] }",
            "struct gui {}\n\nfn main() {\n    let _ = gui{}\n}\n",
            codes::NAMESPACE_NAME_TAKEN,
        ),
        // Two namespaces lifted to one name: `detail` twice.
        (
            "twice",
            "lib = { cpp = \"lib\", namespaces = [\"gui::detail\", \"gui::detail\"] }",
            "fn main() {\n    println!(\"{}\", detail::inner())\n}\n",
            codes::NAMESPACE_NAME_TAKEN,
        ),
        // A namespace the header does not declare.
        (
            "missing",
            "lib = { cpp = \"lib\", namespaces = [\"widgets\"] }",
            "fn main() {\n    println!(\"{}\", lib::global_fn())\n}\n",
            codes::UNRESOLVED_NAME,
        ),
        // A namespace that is a function of the header, not a namespace.
        (
            "notns",
            "lib = { cpp = \"lib\", namespaces = [\"global_fn\"] }",
            "fn main() {\n    println!(\"{}\", lib::global_fn())\n}\n",
            codes::UNRESOLVED_NAME,
        ),
    ];
    for (tag, dep, source, code) in cases {
        let dir = workspace(tag, dep, source);
        let Some(a) = analyse(&dir) else { return };
        let rendered = dz_diag::render_to_string(&a.map, &a.diags);
        assert_eq!(errors(&a), vec![*code], "{tag}:\n{rendered}");
        let manifest = a
            .diags
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .filter_map(|d| d.primary_span())
            .all(|(f, _)| a.map.get(f).path().ends_with("dazzle.toml"));
        assert!(manifest, "{tag}: the error belongs on the manifest:\n{rendered}");
    }
}

/// `namespaces` on a dependency that is not a header: a Dazzle module has no C++
/// namespaces, and its own dependencies are reached with `via`.
#[test]
fn namespaces_takes_a_header() {
    let dir = std::env::temp_dir().join(format!("dz_namespaces_path_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (sub, toml, src) in [
        (
            "app",
            "[module]\nname = \"app\"\nsources = [\"src/**/*.dz\"]\n\n[dependencies]\n\
             util = { path = \"../util\", namespaces = [\"inner\"] }\n\n[build]\noutput = \"executable\"\n",
            "fn main() {\n    util::hello()\n}\n",
        ),
        (
            "util",
            "[module]\nname = \"util\"\nsources = [\"src/**/*.dz\"]\n\n[build]\noutput = \"static-lib\"\n",
            "pub fn hello() {}\n",
        ),
    ] {
        std::fs::create_dir_all(dir.join(sub).join("src")).unwrap();
        std::fs::write(dir.join(sub).join("dazzle.toml"), toml).unwrap();
        std::fs::write(dir.join(sub).join("src/main.dz"), src).unwrap();
    }
    let a = dz_driver::analyse(&dir.join("app"));
    let rendered = dz_diag::render_to_string(&a.map, &a.diags);
    assert_eq!(errors(&a), vec![codes::MANIFEST_BAD_VALUE], "{rendered}");
    assert!(rendered.contains("only a `cpp` dependency takes `namespaces`"), "{rendered}");
}
