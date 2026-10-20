//! The expansion corpus: `tests/expand/*.dz`, each snapshotted as `dz expand` prints it —
//! every invocation replaced by its expansion, hygienic names with their `·N` (spec §12.6).
//!
//! The snapshot is the macro system's output as a reader sees it, so a change to matching,
//! writing or hygiene shows up as a diff of Dazzle source rather than of a tree dump. Each
//! file must also check cleanly: an expansion that reads well but does not compile is not
//! one.

use std::path::{Path, PathBuf};

fn corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/expand")
}

/// The file as resolution sees it, printed by the formatter.
fn expand(path: &Path) -> String {
    let stem = path.file_stem().unwrap().to_string_lossy();
    let dir = std::env::temp_dir().join(format!("dz_expand_{stem}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("dazzle.toml"),
        "[module]\nname = \"expand\"\nsources = [\"src/**/*.dz\"]\n\n[dependencies]\nstd = { open = true }\n",
    )
    .unwrap();
    std::fs::copy(path, dir.join("src/main.dz")).unwrap();
    let a = dz_driver::analyse(&dir);
    let rendered = dz_diag::render_to_string(&a.map, &a.diags);
    assert!(!a.diags.has_errors(), "{stem} does not check:\n{rendered}");
    let pf = a
        .files
        .values()
        .flatten()
        .find(|pf| a.map.get(pf.file).path().ends_with("main.dz"))
        .expect("main.dz");
    let file = a.map.get(pf.file);
    let comments = dz_lex::lex(pf.file, file.text()).comments;
    let synthetic =
        a.expansions.iter().filter(|r| r.file == pf.file).map(|r| r.name_span.start).collect();
    let out = dz_fmt::format_expanded(file, &pf.ast, &comments, &synthetic);
    let _ = std::fs::remove_dir_all(&dir);
    out
}

#[test]
fn every_file_expands_as_its_snapshot_says() {
    let mut files: Vec<PathBuf> = std::fs::read_dir(corpus_dir())
        .expect("tests/expand exists")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "dz"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "the expansion corpus is empty");
    for path in files {
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let out = expand(&path);
        insta::assert_snapshot!(name, out);
    }
}
