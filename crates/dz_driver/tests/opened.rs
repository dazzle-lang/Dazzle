//! What a manifest lets its module name (spec §3.2), across real modules and the whole
//! pipeline: `open`, `[names]`, public dependencies reached by path and by `via`, a file's
//! `{ … }` import, and the mistakes around them. The resolver and the checker must agree on
//! what a bare name means, so a conflict is one error and a name that resolves is also
//! well-typed.
//!
//! Two workspaces, because an error in resolution stops the checker for the whole program:
//! `opened` must check clean all the way through, and `opened_errors` makes each mistake
//! once against the same libraries.

use std::path::{Path, PathBuf};

use dz_diag::{codes, Severity};

fn workspace(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/workspace").join(name)
}

/// Error codes reported in one of the workspace's files, by its file name.
fn errors_in(a: &dz_driver::Analysis, file_name: &str) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = a
        .diags
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .filter(|d| {
            d.primary_span().is_some_and(|(f, _)| {
                a.map.get(f).path().file_name().is_some_and(|n| n == file_name)
            })
        })
        .filter_map(|d| d.code)
        .collect();
    out.sort();
    out
}

#[test]
fn a_manifest_names_what_every_file_uses_and_it_type_checks() {
    let a = dz_driver::analyse(&workspace("opened"));
    let rendered = dz_diag::render_to_string(&a.map, &a.diags);
    assert!(!a.diags.has_errors(), "{rendered}");
    // Checking ran, so the names resolution chose were also typed.
    assert!(a.hir.is_some() && a.mono.is_some(), "{rendered}");
    assert!(!a.diags.iter().any(|d| d.code == Some(codes::UNUSED_IMPORT)), "{rendered}");
}

#[test]
fn each_mistake_is_one_error_where_it_is_made() {
    let a = dz_driver::analyse(&workspace("opened_errors"));
    let rendered = dz_diag::render_to_string(&a.map, &a.diags);
    let mut want = vec![codes::UNRESOLVED_NAME, codes::UNKNOWN_MODULE, codes::AMBIGUOUS_NAME];
    want.sort();
    assert_eq!(errors_in(&a, "conflict.dz"), want, "{rendered}");
    assert_eq!(errors_in(&a, "leaky.dz"), vec![codes::PRIVATE_DEPENDENCY_IN_PUBLIC], "{rendered}");
    // Nothing anywhere else: no cascade into the libraries or the manifests.
    let total = a.diags.iter().filter(|d| d.severity == Severity::Error).count();
    assert_eq!(total, 4, "{rendered}");
}

#[test]
fn the_help_writes_the_fix() {
    let a = dz_driver::analyse(&workspace("opened_errors"));
    let rendered = dz_diag::render_to_string(&a.map, &a.diags);
    // The ambiguity names every way to write it, by key, and the manifest's way to choose.
    assert!(rendered.contains("`std::Vec3` or `geo::Vec3` or `phys::Vec3`"), "{rendered}");
    assert!(rendered.contains("`[names]` `Vec3 = \"std\"`"), "{rendered}");
    // A private name is explained, not suggested back as its own spelling.
    assert!(!rendered.contains("did you mean `secret`"), "{rendered}");
    assert!(rendered.contains("module `geo` has a `secret`"), "{rendered}");
    // A private dependency says which key to make public.
    assert!(rendered.contains("mark `hidden` `pub = true`"), "{rendered}");
}

/// A dependency nothing names is a warning on its manifest line (spec §3.2) — and one that
/// is used only through `[names]`, a `via`, a file's import or an opened name is used; a
/// public one is part of the interface and never reported.
#[test]
fn a_dependency_nothing_uses_is_reported_on_its_manifest_line() {
    let root = std::env::temp_dir().join("dz_driver_unused_dep");
    let _ = std::fs::remove_dir_all(&root);
    let lib = |name: &str, body: &str, deps: &str| {
        let dir = root.join("libs").join(name);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(
            dir.join("dazzle.toml"),
            format!(
                "[module]\nname = \"{name}\"\nsources = [\"src/**/*.dz\"]\n\n[dependencies]\nstd = {{ open = true }}\n{deps}\n[build]\noutput = \"static-lib\"\n"
            ),
        )
        .unwrap();
        std::fs::write(dir.join("src").join(format!("{name}.dz")), body).unwrap();
    };
    lib("inner", "pub fn deep() -> i32 {\n    return 1\n}\n", "");
    lib(
        "outer",
        "pub fn shallow() -> i32 {\n    return 2\n}\n",
        "inner = { path = \"../inner\", pub = true }\n",
    );
    lib("picked", "pub struct Pick {\n    pub a: i32\n}\n", "");
    lib("bare", "pub fn opened_fn() -> i32 {\n    return 3\n}\n", "");
    lib("imported", "pub fn chosen() -> i32 {\n    return 4\n}\n", "");
    lib("spare", "pub fn nobody() -> i32 {\n    return 5\n}\n", "");
    lib("shared", "pub fn interface() -> i32 {\n    return 6\n}\n", "");
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("dazzle.toml"),
        "[module]\nname = \"app\"\nsources = [\"src/**/*.dz\"]\n\n[dependencies]\n\
         std      = { open = true }\n\
         outer    = { path = \"libs/outer\" }\n\
         deeper   = { via = \"outer::inner\" }\n\
         picked   = { path = \"libs/picked\" }\n\
         bare     = { path = \"libs/bare\", open = true }\n\
         imported = { path = \"libs/imported\" }\n\
         spare    = { path = \"libs/spare\" }\n\
         shared   = { path = \"libs/shared\", pub = true }\n\n\
         [names]\nPick = \"picked\"\n\n[build]\noutput = \"static-lib\"\n",
    )
    .unwrap();
    std::fs::write(
        root.join("src/app.dz"),
        "import imported::{ chosen }\n\n\
         fn all() -> i32 {\n    return deeper::deep() + opened_fn() + chosen() + Pick{ a: 1 }.a\n}\n",
    )
    .unwrap();
    let a = dz_driver::analyse(&root);
    let rendered = dz_diag::render_to_string(&a.map, &a.diags);
    let _ = std::fs::remove_dir_all(&root);
    assert!(!a.diags.has_errors(), "{rendered}");
    let unused: Vec<String> = a
        .diags
        .iter()
        .filter(|d| d.code == Some(codes::UNUSED_DEPENDENCY))
        .map(|d| d.message.clone())
        .collect();
    assert_eq!(unused, vec!["dependency `spare` is never used".to_owned()], "{rendered}");
}

/// A module that does not list `std` cannot name what is in it — and is told how to —
/// while the language's own constructs, which use it, still work (spec §14.4).
#[test]
fn a_module_that_does_not_list_std_cannot_name_it() {
    let dir = std::env::temp_dir().join("dz_driver_no_std");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("dazzle.toml"),
        "[module]\nname = \"bare\"\nsources = [\"src/**/*.dz\"]\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/main.dz"),
        "fn total(xs: []i32) -> i32 {\n    let mut t = 0\n    for x in xs {\n        t += x\n    }\n    return t\n}\n\n\
         fn main() {\n    let xs: [3]i32 = [1, 2, 3]\n    println(\"{}\", total(xs))\n    let v = Vec2{}\n}\n",
    )
    .unwrap();
    let a = dz_driver::analyse(&dir);
    let rendered = dz_diag::render_to_string(&a.map, &a.diags);
    let _ = std::fs::remove_dir_all(&dir);
    let errors: Vec<&str> =
        a.diags.iter().filter(|d| d.severity == Severity::Error).filter_map(|d| d.code).collect();
    assert_eq!(errors, vec![codes::UNRESOLVED_NAME], "{rendered}");
    assert!(rendered.contains("add `std = { open = true }`"), "{rendered}");
}
