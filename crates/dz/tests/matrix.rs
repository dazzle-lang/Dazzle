//! Every valid construct in the feature matrix, in every context and both layouts, compiled
//! by a real C++ compiler and run: each placement prints its value, which must be the one
//! the construct promises (see `dz_driver/tests/feature_matrix/mod.rs`). The generated C++
//! must also be warning-clean under every compiler present (invariant 7).

#[path = "../../dz_driver/tests/feature_matrix/mod.rs"]
mod feature_matrix;
#[path = "../../dz_driver/tests/parallel/mod.rs"]
mod parallel;

use std::path::PathBuf;
use std::process::Command;

use feature_matrix::{place, write_project, Layout, CONSTRUCTS, CONTEXTS, LAYOUTS};

fn compilers() -> Vec<&'static str> {
    ["clang++", "g++"]
        .into_iter()
        .filter(
            |c| matches!(Command::new(c).arg("--version").output(), Ok(o) if o.status.success()),
        )
        .collect()
}

/// One program holding every construct in every context, and the lines it must print.
fn program() -> (String, Vec<String>) {
    let mut code = String::new();
    let mut calls = String::new();
    let mut expected = Vec::new();
    for (context_name, context) in CONTEXTS {
        for (construct, body, value) in CONSTRUCTS {
            let name = format!("{construct}_in_{context_name}");
            code.push_str(&place(context, &name, body));
            code.push_str("\n\n");
            calls.push_str(&format!("    #println(\"{name} {{}}\", {name}())\n"));
            expected.push(format!("{name} {value}"));
        }
    }
    code.push_str(&format!("fn main() {{\n{calls}}}\n"));
    (code, expected)
}

fn run_layout(layout: Layout) {
    let (code, expected) = program();
    let dir = std::env::temp_dir().join(format!("dz_matrix_run_{layout:?}"));
    let root = write_project(&dir, layout, &code);

    let out = Command::new(env!("CARGO_BIN_EXE_dz"))
        .args(["run", root.to_str().unwrap()])
        .output()
        .expect("failed to run dz");
    let stdout = String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n");
    assert!(
        out.status.success(),
        "{layout:?}: dz run failed\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let got: Vec<&str> = stdout.lines().collect();
    let wrong: Vec<String> = expected
        .iter()
        .enumerate()
        .filter(|(i, want)| got.get(*i) != Some(&want.as_str()))
        .map(|(i, want)| format!("expected `{want}`, got `{}`", got.get(i).unwrap_or(&"<nothing>")))
        .collect();
    assert!(
        wrong.is_empty(),
        "{layout:?}: {} placements printed the wrong value:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
    assert_eq!(got.len(), expected.len(), "{layout:?}: extra output\n{stdout}");

    // One `-fsyntax-only` per translation unit per compiler, and nothing they share: run
    // them together.
    let checks: Vec<(&'static str, PathBuf)> = compilers()
        .into_iter()
        .flat_map(|cxx| {
            std::fs::read_dir(root.join("build/cpp"))
                .unwrap()
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e == "cpp"))
                .map(move |p| (cxx, p))
        })
        .collect();
    parallel::for_each(&checks, |(cxx, p)| {
        let out = Command::new(cxx)
            .args([
                "-std=c++17",
                "-fno-exceptions",
                "-fno-rtti",
                "-Wall",
                "-Wextra",
                "-fsyntax-only",
            ])
            .arg(p)
            .output()
            .expect("failed to run the C++ compiler");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            out.status.success() && stderr.trim().is_empty(),
            "{layout:?}: {} is not warning-clean under {cxx}:\n{stderr}",
            p.display()
        );
    });
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn every_construct_runs_wherever_it_is_written() {
    if compilers().is_empty() {
        eprintln!("skipping: no C++ compiler available");
        return;
    }
    parallel::for_each(LAYOUTS, |&layout| run_layout(layout));
}
