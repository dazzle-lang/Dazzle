//! `dz check --format=json` and `dz fix` (T-0060): diagnostics a program can read, and the
//! fixes the compiler is certain of applied without anyone reading them.
//!
//! These drive the real binary on a project written to a temporary directory, and never
//! need a C++ compiler.

use std::path::{Path, PathBuf};
use std::process::Command;

fn dz() -> &'static str {
    env!("CARGO_BIN_EXE_dz")
}

/// A project of one file, `src/main.dz`, in a directory of its own.
fn project(name: &str, source: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("dz_fix_{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("dazzle.toml"),
        "[module]\nname = \"fixme\"\nsources = [\"src/**/*.dz\"]\n\n[dependencies]\nstd = { open = true }\n",
    )
    .unwrap();
    std::fs::write(dir.join("src/main.dz"), source).unwrap();
    dir
}

fn json(dir: &Path) -> serde_json::Value {
    let out = Command::new(dz()).args(["check", "--format=json"]).arg(dir).output().unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("not JSON ({e}):\n{text}"))
}

fn source(dir: &Path) -> String {
    std::fs::read_to_string(dir.join("src/main.dz")).unwrap().replace("\r\n", "\n")
}

#[test]
fn json_carries_spans_help_and_fix() {
    let dir = project(
        "json",
        "struct P {\n    pub count: i32\n}\n\nfn main() {\n    let p = P{ count: 1 }\n    println!(\"{}\", p.cuont)\n}\n",
    );
    let v = json(&dir);
    assert_eq!(v["errors"], 1, "{v}");
    let d = &v["diagnostics"][0];
    assert_eq!(d["code"], "DZ0505");
    assert_eq!(d["severity"], "error");
    assert_eq!((d["line"].as_u64(), d["column"].as_u64()), (Some(7), Some(22)));
    assert_eq!(d["help"][0], "did you mean `count`?");
    // A guess is offered, never certain.
    assert_eq!(d["fix"]["certain"], false);
    let edit = &d["fix"]["edits"][0];
    assert_eq!(edit["text"], "count");
    let text = source(&dir);
    let (start, end) = (edit["start"].as_u64().unwrap(), edit["end"].as_u64().unwrap());
    assert_eq!(&text[start as usize..end as usize], "cuont", "byte offsets into the file");
}

#[test]
fn a_clean_project_is_an_empty_list() {
    let dir = project("clean", "fn main() {\n    println!(\"hi\")\n}\n");
    let v = json(&dir);
    assert_eq!(v["diagnostics"].as_array().map(Vec::len), Some(0), "{v}");
    assert_eq!(v["errors"], 0);
}

#[test]
fn fix_applies_what_is_certain_and_leaves_guesses() {
    let dir = project(
        "apply",
        "struct C {\n    pub n: i32\n\n    pub mut fn bump(&mut self) {\n        n += 1\n    }\n}\n\n\
         fn main() {\n    let mut c = C{ n: 0 }\n    c.bump()\n    #println(\"{}\", c.n)\n}\n",
    );
    let dry = Command::new(dz()).args(["fix", "--dry-run"]).arg(&dir).output().unwrap();
    assert!(String::from_utf8_lossy(&dry.stdout).contains("would apply 2 fixes"));
    assert!(source(&dir).contains("mut fn"), "a dry run changes nothing");

    let out = Command::new(dz()).arg("fix").arg(&dir).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stdout));
    let text = source(&dir);
    assert!(text.contains("    pub fn bump(&mut self) {"), "{text}");
    assert!(text.contains("    println!(\"{}\", c.n)"), "{text}");

    // What the fixes uncovered: flow analysis now runs, and the unused variable it finds has
    // a certain fix of its own.
    std::fs::write(
        dir.join("src/main.dz"),
        text.replace("fn main() {\n", "fn main() {\n    let spare = 3\n"),
    )
    .unwrap();
    Command::new(dz()).arg("fix").arg(&dir).output().unwrap();
    assert!(source(&dir).contains("let _spare = 3"), "{}", source(&dir));
    assert_eq!(json(&dir)["diagnostics"].as_array().map(Vec::len), Some(0));
}

#[test]
fn fix_does_not_apply_a_guess() {
    let dir = project(
        "guess",
        "struct P {\n    pub count: i32\n}\n\nfn main() {\n    let p = P{ count: 1 }\n    println!(\"{}\", p.cuont)\n}\n",
    );
    let out = Command::new(dz()).arg("fix").arg(&dir).output().unwrap();
    assert!(!out.status.success(), "the error is still there");
    assert!(source(&dir).contains("p.cuont"));
}
