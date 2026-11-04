//! `dz new`: the project it writes loads and checks with no diagnostics, and it refuses a
//! folder it cannot name or would overwrite. Checked, not built, so no C++ compiler is
//! needed.

use std::path::PathBuf;
use std::process::{Command, Output};

fn dz(args: &[&str], dir: &PathBuf) -> Output {
    Command::new(env!("CARGO_BIN_EXE_dz")).args(args).current_dir(dir).output().unwrap()
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("dz_new_{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn new_project_checks_clean() {
    let dir = scratch("clean");
    let out = dz(&["new", "my-game"], &dir);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let manifest = std::fs::read_to_string(dir.join("my-game/dazzle.toml")).unwrap();
    // A `-` is not allowed in a module name, so it is read as `_`.
    assert!(manifest.contains("name    = \"my_game\""), "{manifest}");
    let out = dz(&["check", "my-game"], &dir);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(out.stderr.is_empty(), "{}", String::from_utf8_lossy(&out.stderr));
    // The first file a newcomer opens is held to `dz fmt`, as the examples are.
    let out = dz(&["fmt", "--check", "my-game/src/main.dz"], &dir);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stdout));
}

#[test]
fn new_in_the_current_folder() {
    let dir = scratch("here").join("hello");
    std::fs::create_dir_all(&dir).unwrap();
    let out = dz(&["new", "."], &dir);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let manifest = std::fs::read_to_string(dir.join("dazzle.toml")).unwrap();
    assert!(manifest.contains("name    = \"hello\""), "{manifest}");
}

#[test]
fn new_refuses_what_it_cannot_name_or_would_overwrite() {
    let dir = scratch("refused");
    for name in ["fn", "std", "9lives"] {
        let out = dz(&["new", name], &dir);
        assert!(!out.status.success(), "`dz new {name}` should be refused");
        assert!(!dir.join(name).exists(), "`dz new {name}` left a folder behind");
    }
    assert!(dz(&["new", "game"], &dir).status.success());
    let out = dz(&["new", "game"], &dir);
    assert!(!out.status.success(), "a second `dz new game` should be refused");
}
