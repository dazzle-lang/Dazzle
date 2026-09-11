//! End-to-end tests: Dazzle source through generated C++ to a running program (spec §17.5).
//!
//! These drive the real `dz` binary and invoke a real C++ compiler, so they are the only
//! tests that prove the whole pipeline works. Each project under `tests/run/` carries an
//! `expected.txt` holding the exact stdout its program must produce.

use std::path::{Path, PathBuf};
use std::process::Command;

fn dz() -> &'static str {
    env!("CARGO_BIN_EXE_dz")
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn run_dir(name: &str) -> PathBuf {
    repo_root().join("tests/run").join(name)
}

/// Every project under `tests/run/` that has an `expected.txt`.
fn projects() -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    let root = repo_root().join("tests/run");
    let Ok(entries) = std::fs::read_dir(&root) else { return out };
    for e in entries.flatten() {
        let p = e.path();
        if p.join("expected.txt").exists() {
            out.push((p.file_name().unwrap().to_string_lossy().to_string(), p));
        }
    }
    out.sort();
    out
}

fn have_compiler() -> bool {
    if let Ok(cxx) = std::env::var("DZ_CXX") {
        if !cxx.is_empty() {
            return true;
        }
    }
    ["clang++", "g++", "c++"]
        .iter()
        .any(|c| matches!(Command::new(c).arg("--version").output(), Ok(o) if o.status.success()))
}

fn normalise(s: &str) -> String {
    s.replace("\r\n", "\n").trim_end().to_owned()
}

#[test]
fn every_program_produces_its_expected_output() {
    if !have_compiler() {
        eprintln!("skipping: no C++ compiler available");
        return;
    }
    let found = projects();
    assert!(!found.is_empty(), "no runnable projects found under tests/run/");

    for (name, dir) in found {
        // Projects that are meant to abort record their expectation on the first line.
        let expected = std::fs::read_to_string(dir.join("expected.txt")).unwrap();
        let should_panic = expected.starts_with("#panic");
        let expected_body = normalise(expected.strip_prefix("#panic\n").unwrap_or(&expected));

        let out = Command::new(dz())
            .args(["run", dir.to_str().unwrap()])
            .output()
            .expect("failed to run dz");

        let stdout = normalise(&String::from_utf8_lossy(&out.stdout));
        let stderr = String::from_utf8_lossy(&out.stderr).to_string();

        if should_panic {
            assert!(
                !out.status.success(),
                "{name}: expected a panic but the program succeeded\n{stderr}"
            );
            for line in expected_body.lines() {
                assert!(
                    stderr.contains(line.trim()) || stdout.contains(line.trim()),
                    "{name}: expected output to contain {line:?}\nstdout:\n{stdout}\nstderr:\n{stderr}"
                );
            }
        } else {
            assert!(out.status.success(), "{name}: dz run failed\n{stderr}");
            assert_eq!(stdout, expected_body, "{name}: stdout differs");
        }
    }
}

#[test]
fn hello_world_runs() {
    // The M4 acceptance criterion, called out on its own so a failure names itself.
    if !have_compiler() {
        return;
    }
    let out = Command::new(dz())
        .args(["run", run_dir("hello").to_str().unwrap()])
        .output()
        .expect("failed to run dz");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(normalise(&String::from_utf8_lossy(&out.stdout)), "Hello, world!");
}

#[test]
fn generated_cpp_carries_line_directives_back_to_dazzle() {
    // spec §16.3 — this is what makes breakpoints, stack traces and any C++ error that
    // slips through point at `.dz` source.
    let out = Command::new(dz())
        .args(["cpp", run_dir("hello").to_str().unwrap()])
        .output()
        .expect("failed to run dz");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));

    let cpp = std::fs::read_to_string(run_dir("hello").join("build/cpp/hello.cpp"))
        .expect("generated hello.cpp");
    assert!(cpp.contains("#line "), "no #line directives:\n{cpp}");
    assert!(cpp.contains("main.dz"), "#line does not name the .dz file:\n{cpp}");
    assert!(cpp.contains("namespace dz"), "output is not in the dz namespace:\n{cpp}");
}

#[test]
fn generated_cpp_snapshot() {
    let out = Command::new(dz())
        .args(["cpp", run_dir("hello").to_str().unwrap()])
        .output()
        .expect("failed to run dz");
    assert!(out.status.success());

    let dir = run_dir("hello").join("build/cpp");
    let cpp = std::fs::read_to_string(dir.join("hello.cpp")).unwrap();
    let header = std::fs::read_to_string(dir.join("hello.h")).unwrap();
    // `#line` names the `.dz` file by absolute path, which differs on every machine and
    // cannot be stripped by matching the repo root (`repo_root()` is not normalised, and
    // drive letters and case vary). Keep only the file name.
    let cpp = cpp
        .replace('\\', "/")
        .lines()
        .map(|l| match l.strip_prefix("#line ") {
            Some(rest) => match (rest.find(" \""), rest.rfind('/')) {
                (Some(n), Some(slash)) => format!("#line {}\"{}", &rest[..=n], &rest[slash + 1..]),
                _ => l.to_owned(),
            },
            None => l.to_owned(),
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    insta::assert_snapshot!("hello_cpp", format!("{header}\n// ---\n{cpp}"));
}

#[test]
fn the_generated_code_builds_clean_under_wall_wextra() {
    // spec §16.5 — nothing generated may need exceptions or RTTI, and it must not warn.
    let Some(cxx) = ["clang++", "g++"]
        .iter()
        .find(|c| matches!(Command::new(c).arg("--version").output(), Ok(o) if o.status.success()))
    else {
        return;
    };

    // Every run fixture, not just one: `ownership` is the only program that exercises
    // the generated special members, and `failure` the only one that exercises `or`,
    // tagged unions and tuples. A warning in generated code is a compiler bug
    // (invariant 7), so the check has to see all of it.
    for fixture in ["features", "ownership", "failure", "hello"] {
        check_warning_clean(cxx, fixture);
    }
}

/// Generate one fixture's C++ and compile every translation unit with warnings as errors.
fn check_warning_clean(cxx: &str, fixture: &str) {
    // Copied into a scratch project: the run test drives the same directories, and cargo
    // runs tests in parallel, so sharing a build directory would race.
    let source = std::fs::read_to_string(run_dir(fixture).join("src/main.dz")).unwrap();
    let manifest = format!("[module]
name = \"{fixture}\"
sources = [\"src/**/*.dz\"]
");
    let dir = scratch(&format!("dz_wall_wextra_{fixture}"), &manifest, &source);
    let out = Command::new(dz())
        .args(["cpp", dir.to_str().unwrap()])
        .output()
        .expect("failed to run dz");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));

    let dir = dir.join("build/cpp");
    for entry in std::fs::read_dir(&dir).unwrap().flatten() {
        let p = entry.path();
        if p.extension().is_some_and(|e| e == "cpp") {
            let out = Command::new(cxx)
                .args(["-std=c++17", "-fno-exceptions", "-fno-rtti", "-Wall", "-Wextra", "-fsyntax-only"])
                .arg(&p)
                .output()
                .expect("failed to run the C++ compiler");
            let stderr = String::from_utf8_lossy(&out.stderr);
            assert!(
                out.status.success() && stderr.trim().is_empty(),
                "{} is not warning-clean under {cxx}:\n{stderr}",
                p.display()
            );
        }
    }
}

#[test]
fn unsupported_constructs_are_reported_not_miscompiled() {
    // Codegen must refuse what it cannot emit, rather than producing something subtly
    // wrong. The deferral register in docs/PROGRESS.md tracks these.
    let dir = std::env::temp_dir().join("dz_m4_unsupported");
    let _ = std::fs::create_dir_all(dir.join("src"));
    std::fs::write(
        dir.join("dazzle.toml"),
        "[module]\nname = \"u\"\nsources = [\"src/**/*.dz\"]\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/a.dz"),
        // A trait object: dynamic dispatch (spec §7.6) is not in M6, and without a DZ0801
        // it would reach C++ as a pointer to a type nobody declared.
        "trait Shape {\n    fn area(&self) -> f32\n}\n\n\
         fn total(s: ^Shape) -> f32 {\n    return 0\n}\n\n\
         fn main() {\n    println(\"{}\", 1)\n}\n",
    )
    .unwrap();

    let out = Command::new(dz())
        .args(["cpp", dir.to_str().unwrap()])
        .output()
        .expect("failed to run dz");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "expected codegen to refuse:\n{stderr}");
    assert!(stderr.contains("DZ0801"), "expected DZ0801:\n{stderr}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn runaway_instantiation_is_bounded_by_the_compiler() {
    // Spec §8.4: recursive instantiation depth is bounded by the compiler with a clear
    // diagnostic, rather than left to C++'s template depth limit — and one runaway is one
    // diagnostic, not one per level.
    let dir = scratch(
        "dz_mono_runaway",
        "[module]\nname = \"r\"\nsources = [\"src/**/*.dz\"]\n",
        "struct Box<T> {\n    pub v: T\n}\n\n\
         fn deeper<T>(x: T) -> i32 {\n    let b = Box<T>{ v: move(x) }\n    \
         return deeper(move(b))\n}\n\n\
         fn main() {\n    println(\"{}\", deeper(1))\n}\n",
    );
    let out = Command::new(dz()).args(["check", dir.to_str().unwrap()]).output().unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "expected the runaway to be refused:\n{stderr}");
    assert_eq!(stderr.matches("DZ0536").count(), 1, "expected exactly one DZ0536:\n{stderr}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_project_without_main_is_reported() {
    let dir = std::env::temp_dir().join("dz_m4_nomain");
    let _ = std::fs::create_dir_all(dir.join("src"));
    std::fs::write(
        dir.join("dazzle.toml"),
        "[module]\nname = \"n\"\nsources = [\"src/**/*.dz\"]\n",
    )
    .unwrap();
    std::fs::write(dir.join("src/a.dz"), "fn helper() -> i32 {\n    return 1\n}\n").unwrap();

    let out = Command::new(dz())
        .args(["cpp", dir.to_str().unwrap()])
        .output()
        .expect("failed to run dz");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("DZ0802"), "expected DZ0802:\n{stderr}");
    let _ = std::fs::remove_dir_all(&dir);
}

// --- code quality ---------------------------------------------------------

/// Write a throwaway project and return its directory.
fn scratch(name: &str, manifest: &str, source: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("dazzle.toml"), manifest).unwrap();
    std::fs::write(dir.join("src/main.dz"), source).unwrap();
    dir
}

const LITERAL_SRC: &str = "\
pub struct Vec3 {
    pub x: f32
    pub y: f32
    pub z: f32 = 0
}

fn main() {
    let a = Vec3{ y: 2, x: 1 }
    println(\"{} {}\", a.x, a.y)
}
";

#[test]
fn cpp20_uses_designated_initialisers_and_cpp17_does_not() {
    for (std_name, expect_designated) in [("c++17", false), ("c++20", true)] {
        let manifest = format!(
            "[module]\nname = \"lit\"\nsources = [\"src/**/*.dz\"]\n\n\
             [build]\noutput = \"executable\"\ncpp_standard = \"{std_name}\"\n"
        );
        let dir = scratch(&format!("dz_lit_{}", std_name.replace("+", "p")), &manifest, LITERAL_SRC);
        let out = Command::new(dz()).args(["cpp", dir.to_str().unwrap()]).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));

        let cpp = std::fs::read_to_string(dir.join("build/cpp/lit.cpp")).unwrap();
        if expect_designated {
            // ...and reordered into declaration order, which C++20 requires.
            // The literals carry their `f32` type: the HIR records what the checker
            // concluded, so codegen no longer leans on a C++ implicit conversion.
            assert!(
                cpp.contains("Vec3{ .x = 1.0f, .y = 2.0f }"),
                "expected designated init:\n{cpp}"
            );
        } else {
            // Match the designated form exactly: the lambda form legitimately contains
            // `_dz_lit1.x = 1`, which has `.x = 1` as a substring.
            assert!(!cpp.contains("Vec3{ ."), "C++17 cannot use designated init:\n{cpp}");
            assert!(cpp.contains("_dz_lit"), "expected the lambda form:\n{cpp}");
        }

        // Whichever form is used, the program must behave the same.
        if have_compiler() {
            let run = Command::new(dz()).args(["run", dir.to_str().unwrap()]).output().unwrap();
            assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
            assert_eq!(normalise(&String::from_utf8_lossy(&run.stdout)), "1 2");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn the_access_helpers_cost_nothing_in_the_generated_code() {
    // `dz::obj` and `dz::index` exist so codegen can emit one form for values, references
    // and pointers. They are marked always-inline precisely so they do not survive into
    // the object code — including at -O0, because a game has to stay playable in a debug
    // build. This test fails if that guarantee is ever lost.
    let Some(cxx) = ["clang++", "g++"]
        .iter()
        .find(|c| matches!(Command::new(c).arg("--version").output(), Ok(o) if o.status.success()))
    else {
        return;
    };

    let dir = scratch(
        "dz_zero_overhead",
        "[module]\nname = \"perf\"\nsources = [\"src/**/*.dz\"]\n",
        "fn sum(xs: []i32) -> i32 {\n    let mut total = 0\n    for i in 0..xs.len {\n        \
         total += xs[i]\n    }\n    return total\n}\n\nfn main() {\n    println(\"{}\", 0)\n}\n",
    );
    let out = Command::new(dz()).args(["cpp", dir.to_str().unwrap()]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));

    for opt in ["-O0", "-O2"] {
        let asm = dir.join(format!("perf{opt}.s"));
        let out = Command::new(cxx)
            .args(["-std=c++17", "-fno-exceptions", "-fno-rtti", opt, "-S"])
            .arg(dir.join("build/cpp/perf.cpp"))
            .arg("-o")
            .arg(&asm)
            .output()
            .expect("failed to run the C++ compiler");
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));

        let text = std::fs::read_to_string(&asm).unwrap();
        let leaked: Vec<&str> = text
            .lines()
            .filter(|l| l.contains("call"))
            .filter(|l| {
                // `panic_bounds` is the cold failure path and should stay a call.
                (l.contains("obj@") || l.contains("3obj") || l.contains("index@") || l.contains("5index"))
                    && !l.contains("panic")
            })
            .collect();
        assert!(
            leaked.is_empty(),
            "access helpers were not inlined at {opt} under {cxx}:\n{}",
            leaked.join("\n")
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}
