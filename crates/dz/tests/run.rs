//! End-to-end tests: Dazzle source through generated C++ to a running program (spec §17.5).
//!
//! These drive the real `dz` binary and invoke a real C++ compiler, so they are the only
//! tests that prove the whole pipeline works. Each project under `tests/run/` carries an
//! `expected.txt` holding the exact stdout its program must produce.

use std::path::{Path, PathBuf};
use std::process::Command;

#[path = "../../dz_driver/tests/parallel/mod.rs"]
mod parallel;

fn dz() -> &'static str {
    env!("CARGO_BIN_EXE_dz")
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn run_dir(name: &str) -> PathBuf {
    repo_root().join("tests/run").join(name)
}

/// Every project under `examples/`: the programs that open a window — `raylib` and `rgfw`,
/// M7b's acceptance criterion, and `breakout`, M12's — and so have no `expected.txt` to
/// compare against, plus any other example. They are built rather than run; see
/// `every_example_builds`.
fn example_projects() -> Vec<(String, PathBuf)> {
    let root = repo_root().join("examples");
    let Ok(entries) = std::fs::read_dir(&root) else { return Vec::new() };
    let mut out: Vec<(String, PathBuf)> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("dazzle.toml").exists())
        .map(|p| (p.file_name().unwrap().to_string_lossy().to_string(), p))
        .collect();
    out.sort();
    out
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

/// A project with a recipe that names its CMake build (spec §15.1, D-0090).
fn needs_cmake(dir: &Path) -> bool {
    std::fs::read_to_string(dir.join("dazzle.toml"))
        .is_ok_and(|t| t.lines().any(|l| l.trim_start().starts_with("cmake ")))
}

/// CMake is needed only by a project that has such a recipe, so a machine without it still
/// runs every other test.
fn have_cmake() -> bool {
    matches!(Command::new("cmake").arg("--version").output(), Ok(o) if o.status.success())
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

    // Each project has a build directory of its own, so they are built and run on every
    // core rather than one after another: the C++ compiler is what this costs, and it is
    // one process per project either way.
    parallel::for_each(&found, |(name, dir)| {
        // Projects that are meant to abort record their expectation on the first line.
        //
        // Normalised *before* the marker is stripped: git may check these files out with
        // CRLF, and `panic!\r\n` does not start with `panic!\n`. Stripping first made the
        // marker part of the expected output, and the test then looked for the literal
        // "panic!" in the program's own stderr — which is how this passed on a Linux
        // checkout and failed on a Windows one.
        let expected = normalise(&std::fs::read_to_string(dir.join("expected.txt")).unwrap());
        let should_panic = expected.starts_with("panic!");
        let expected_body =
            expected.strip_prefix("panic!\n").map(str::to_owned).unwrap_or(expected);
        if needs_cmake(dir) && !have_cmake() {
            eprintln!("skipping {name}: it builds a library with CMake, which is not installed");
            return;
        }

        // The default build, scope regions on (T-0152), whatever the environment says.
        let out = Command::new(dz())
            .args(["run", dir.to_str().unwrap()])
            .env_remove("DZ_REGIONS")
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
    });
}

/// Scope regions (spec §14.6, T-0117) change where memory comes from and nothing else, so
/// every program here prints its `expected.txt` with them off too (`DZ_REGIONS=0`, every
/// value on the heap), and that C++ is warning-clean on every compiler present (invariant
/// 7). The default build, regions on, is what the other tests here run (T-0152). The switch
/// is set here rather than inherited, so the test means the same everywhere; each program
/// is a copy, so it does not race the run test building the same directory.
#[test]
fn regions_change_nothing() {
    if !have_compiler() {
        eprintln!("skipping: no C++ compiler available");
        return;
    }
    let compilers: Vec<&str> = ["clang++", "g++"]
        .into_iter()
        .filter(
            |c| matches!(Command::new(c).arg("--version").output(), Ok(o) if o.status.success()),
        )
        .collect();
    let found = projects();
    parallel::for_each(&found, |(name, dir)| {
        if needs_cmake(dir) && !have_cmake() {
            eprintln!("skipping {name}: it builds a library with CMake, which is not installed");
            return;
        }
        let expected = normalise(&std::fs::read_to_string(dir.join("expected.txt")).unwrap());
        let copy = copy_fixture(name, &format!("dz_heap_{name}"));
        let out = Command::new(dz())
            .args(["run", copy.to_str().unwrap()])
            .env("DZ_REGIONS", "0")
            .output()
            .expect("failed to run dz");
        let stdout = normalise(&String::from_utf8_lossy(&out.stdout));
        let stderr = String::from_utf8_lossy(&out.stderr).to_string();
        match expected.strip_prefix("panic!\n") {
            Some(body) => {
                assert!(!out.status.success(), "{name}: expected a panic with regions off");
                for line in body.lines() {
                    assert!(
                        stderr.contains(line.trim()) || stdout.contains(line.trim()),
                        "{name}: expected {line:?} with regions off\nstdout:\n{stdout}\nstderr:\n{stderr}"
                    );
                }
            }
            None => {
                assert!(out.status.success(), "{name}: dz run failed with regions off\n{stderr}");
                assert_eq!(stdout, expected, "{name}: stdout differs with regions off");
            }
        }
        if WARNING_CLEAN.contains(&name.as_str()) {
            for cxx in &compilers {
                assert_cpp_warning_clean(cxx, &copy.join("build/cpp"));
            }
        }
        let _ = std::fs::remove_dir_all(&copy);
    });
}

/// Every program here, built with clang++ under AddressSanitizer and UBSan, regions on and
/// off, prints its `expected.txt` and draws no report (T-0132, D-0094). The checker's
/// promise is that safe code reaches no undefined behaviour (spec §1.1); this is the
/// evidence, and a wrong placement especially is a use after free that may still print
/// the right answer. Ignored by default, since it needs clang++ with the sanitizer runtimes
/// and doubles the run corpus's time: CI's `sanitizers` job runs it with `--ignored`.
#[test]
#[ignore]
fn every_program_is_clean_under_sanitizers() {
    let found = projects();
    let runs: Vec<(String, &str)> =
        found.iter().flat_map(|(name, _)| [(name.clone(), "1"), (name.clone(), "0")]).collect();
    parallel::for_each(&runs, |(name, regions)| {
        let dir = run_dir(name);
        if needs_cmake(&dir) && !have_cmake() {
            eprintln!("skipping {name}: it builds a library with CMake, which is not installed");
            return;
        }
        let expected = normalise(&std::fs::read_to_string(dir.join("expected.txt")).unwrap());
        let copy = copy_fixture(name, &format!("dz_san_{name}_{regions}"));
        let what = format!("{name} (regions {})", if *regions == "1" { "on" } else { "off" });
        let out = sanitized_run(&what, &copy, regions);
        let stdout = normalise(&String::from_utf8_lossy(&out.stdout));
        let stderr = String::from_utf8_lossy(&out.stderr).to_string();
        match expected.strip_prefix("panic!\n") {
            Some(body) => {
                assert!(!out.status.success(), "{what}: expected a panic\n{stderr}");
                for line in body.lines() {
                    assert!(
                        stderr.contains(line.trim()) || stdout.contains(line.trim()),
                        "{what}: expected {line:?}\nstdout:\n{stdout}\nstderr:\n{stderr}"
                    );
                }
            }
            None => {
                assert!(out.status.success(), "{what}: dz run failed\n{stderr}");
                assert_eq!(stdout, expected, "{what}: stdout differs");
            }
        }
        let _ = std::fs::remove_dir_all(&copy);
    });
}

/// The examples that open no window or audio device, under the same sanitizers (T-0132).
/// They have no `expected.txt`: running to the end without a report is the test. Each runs
/// in its own folder, where `stb` writes the `render.png` it is ignored for.
#[test]
#[ignore]
fn headless_examples_are_clean_under_sanitizers() {
    for name in ["ownership", "stb"] {
        let dir = repo_root().join("examples").join(name);
        for regions in ["1", "0"] {
            let what = format!("examples/{name} (regions {regions})");
            let out = sanitized_run(&what, &dir, regions);
            let stderr = String::from_utf8_lossy(&out.stderr);
            assert!(out.status.success(), "{what}: dz run failed\n{stderr}");
        }
    }
}

/// The ties check against the sanitizers on programs no one wrote by hand (T-0134): a fixed
/// seed of `tools/fuzz_ties.py`, so a change to the checker that accepts a program reaching
/// freed memory, of a shape the generator writes, fails here. A fixed seed rather than a new
/// one each run, so a failure belongs to the change that caused it; more seeds are run by
/// hand. Needs Python 3, and a debug `dz`, which `cargo test` builds.
#[test]
#[ignore]
fn ties_fuzzer_finds_no_hole_under_sanitizers() {
    let cxx = std::env::var("DZ_CXX").ok().filter(|c| !c.is_empty()).unwrap_or("clang++".into());
    let work = repo_root().join("target").join("fuzz_ties_ci");
    let out = Command::new("python3")
        .arg(repo_root().join("tools").join("fuzz_ties.py"))
        .args(["--cases", "200", "--batch", "100", "--seed", "1", "--cxx", &cxx, "--dz", dz()])
        .arg("--work")
        .arg(&work)
        .output()
        .expect("failed to run python3 tools/fuzz_ties.py");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "the ties fuzzer found a hole\n{stdout}\n{stderr}");
}

/// `dz run` on `dir` with clang++ (or `DZ_CXX`) under AddressSanitizer and UBSan, failing
/// on any report. `regions` is `DZ_REGIONS`: "1" for scope regions, "0" for the heap only.
fn sanitized_run(what: &str, dir: &Path, regions: &str) -> std::process::Output {
    let cxx = std::env::var("DZ_CXX").ok().filter(|c| !c.is_empty()).unwrap_or("clang++".into());
    let flags =
        "-fsanitize=address,undefined -fno-sanitize-recover=all -fno-omit-frame-pointer -g -O1";
    let out = Command::new(dz())
        .args(["run", dir.to_str().unwrap()])
        .current_dir(dir)
        .env("DZ_CXX", &cxx)
        .env("DZ_CXXFLAGS", flags)
        .env("DZ_REGIONS", regions)
        // A leak is not undefined behaviour, and a program that panics leaks by design.
        .env("ASAN_OPTIONS", "detect_leaks=0")
        .env("UBSAN_OPTIONS", "print_stacktrace=1")
        .output()
        .expect("failed to run dz");
    let stderr = String::from_utf8_lossy(&out.stderr);
    for report in ["AddressSanitizer", "runtime error:", "UndefinedBehaviorSanitizer"] {
        assert!(!stderr.contains(report), "{what}: the sanitizer reported\n{stderr}");
    }
    out
}

/// `dz placements` prints every site in the project's own files with the editor hint's
/// label, and none of `std`'s; `--format=json` says the same for a program to read
/// (spec §14.6, T-0117). It only analyses, so it needs no C++ compiler.
#[test]
fn dz_placements_prints_each_site() {
    let dir = run_dir("regions");
    let out = Command::new(dz())
        .args(["placements", dir.to_str().unwrap()])
        .output()
        .expect("failed to run dz");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = normalise(&String::from_utf8_lossy(&out.stdout));
    let lines: Vec<&str> = text.lines().collect();
    assert!(lines.iter().all(|l| !l.contains("<std>")), "{text}");
    assert!(
        lines[0].ends_with("src/main.dz:14:18: List<i32>  region: `sum_squares`; drop skipped"),
        "{text}"
    );
    assert!(
        lines[1].ends_with("src/main.dz:27:18: List<i32>  result: where `evens`'s caller keeps it"),
        "{text}"
    );
    assert!(
        lines[8].ends_with("src/main.dz:82:22: String::from  heap: passed to `store.names.push`"),
        "{text}"
    );
    assert_eq!(lines.last().copied(), Some("91 allocations, 62 in a region"), "{text}");

    let out = Command::new(dz())
        .args(["placements", "--format=json", dir.to_str().unwrap()])
        .output()
        .expect("failed to run dz");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let doc: serde_json::Value = serde_json::from_slice(&out.stdout).expect("JSON on stdout");
    let sites = doc["sites"].as_array().unwrap();
    assert_eq!(sites.len(), 91);
    assert_eq!(sites[0]["placement"], "region");
    assert_eq!(sites[0]["scope"], "`sum_squares`");
    assert_eq!(sites[0]["line"], 14);
    // Its region frees the list whole, so its drop is skipped (T-0118).
    assert_eq!(sites[0]["drop_skipped"], true);
    assert_eq!(sites[1]["placement"], "result");
    assert_eq!(sites[1]["function"], "`evens`");
    assert_eq!(sites[8]["placement"], "heap");
    assert_eq!(sites[8]["why"], "passed to `store.names.push`");
    assert_eq!(sites[8]["label"], "heap: passed to `store.names.push`");
    assert_eq!(sites[8]["drop_skipped"], false);
    assert!(sites[8]["at"]["line"].is_u64(), "a heap decision says where: {}", sites[8]);
}

/// The examples, guarded. Most open a window or an audio device, so none can carry an
/// `expected.txt`; without this, M7b's own criterion would go
/// untested — a break in `import cpp`, in a `[cpp.libs]` recipe, in the single-header
/// `header =` form, in callbacks or in `[cpp.instantiate]` would pass green — and an
/// example could rot unnoticed, as the pre-spec sketches once did. `dz build` exercises
/// all of it — libclang reading the headers, the library's own sources compiling, the link
/// — and stops short of the window.
#[test]
fn every_example_builds() {
    if !have_compiler() {
        eprintln!("skipping: no C++ compiler available");
        return;
    }
    let found = example_projects();
    assert!(found.len() >= 3, "expected the examples under examples/, found {found:?}");
    // The libraries are git submodules. A checkout without them would fail below with a
    // missing-file error from deep inside a recipe; say what is actually wrong.
    for lib in [
        "raylib/src/raylib.h",
        "SDL/include/SDL3/SDL.h",
        "RGFW/RGFW.h",
        "sokol/sokol_app.h",
        "stb/stb_image_write.h",
        "miniaudio/miniaudio.h",
        "box2d/include/box2d/box2d.h",
    ] {
        assert!(
            repo_root().join("examples/vendor").join(lib).exists(),
            "examples/vendor/{lib} is missing: run `git submodule update --init`"
        );
    }

    for (name, dir) in found {
        if needs_cmake(&dir) && !have_cmake() {
            eprintln!("skipping {name}: it builds a library with CMake, which is not installed");
            continue;
        }
        let out = Command::new(dz())
            .args(["build", dir.to_str().unwrap()])
            .output()
            .expect("failed to run dz");
        assert!(
            out.status.success(),
            "{name}: dz build failed\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

/// The build mode and `--flag` choose which `#if` branches exist (spec §12.10): the same
/// `tests/run/flags` program, built `--release` with its own `CHEATS` on, takes the other
/// branch at every level — declarations, statements, and a chain's `#else #if`.
#[test]
fn build_flags_choose_the_branches() {
    if !have_compiler() {
        eprintln!("skipping: no C++ compiler available");
        return;
    }
    let dir = copy_fixture("flags", "dz_build_flags");
    let out = Command::new(dz())
        .args(["run", dir.to_str().unwrap(), "--release", "--flag", "CHEATS"])
        .output()
        .expect("failed to run dz");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "dz run failed\n{stderr}");
    assert_eq!(
        normalise(&String::from_utf8_lossy(&out.stdout)),
        "99 14\noptimised build\non a known platform\nprofiled frame"
    );

    // A flag nobody registered is refused, not quietly off.
    let bad = Command::new(dz())
        .args(["check", dir.to_str().unwrap(), "--flag", "CHEAT"])
        .output()
        .expect("failed to run dz");
    assert!(!bad.status.success());
    assert!(String::from_utf8_lossy(&bad.stderr).contains("DZ0309"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Integer overflow wraps in every build (spec §5.1, 18.63), including at `-O2`, where an
/// optimiser assuming signed overflow never happens would fold `x + 1 > x` to `true`. And
/// `or unreachable` panics in every build; only inside `unsafe`, a block or an `unsafe fn`,
/// may an optimised build assume success (§9.4, D-0020, D-0094 point 4). The assumption is
/// checked in the generated C++, since the behaviour is undefined when it is wrong.
#[test]
fn overflow_wraps_and_unreachable_is_assumed_only_in_unsafe() {
    if !have_compiler() {
        eprintln!("skipping: no C++ compiler available");
        return;
    }
    let dir = scratch(
        "dz_overflow",
        "[module]\nname = \"overflow\"\nsources = [\"src/**/*.dz\"]\n",
        "fn grows(x: i32) -> bool {\n    return x + 1 > x\n}\n\nfn find(n: i32) -> i32? {\n    if n > 0 {\n        return n\n    }\n    return none\n}\n\nunsafe fn first(n: i32) -> i32 {\n    return find(n) or unreachable\n}\n\nfn main() {\n    let max: i32 = 2147483647\n    let small = 250u8\n    let v = find(3) or unreachable\n    let w = unsafe { find(4) or unreachable }\n    let f = unsafe { first(5) }\n    println!(\"{} {} {} {} {} {}\", max + 1, grows(max), small + 10, v, w, f)\n}\n",
    );
    for mode in ["--debug", "--release", "--shipping"] {
        let out = Command::new(dz())
            .args(["run", dir.to_str().unwrap(), mode])
            .output()
            .expect("failed to run dz");
        assert!(out.status.success(), "{mode}: {}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(
            normalise(&String::from_utf8_lossy(&out.stdout)),
            "-2147483648 false 4 3 4 5",
            "{mode}"
        );
        // Two assumptions in an optimised build, the block's and the `unsafe fn`'s; the
        // safe one in `main` panics in every build.
        let cpp = std::fs::read_to_string(dir.join("build/cpp/overflow.cpp")).unwrap();
        let assumed = if mode == "--debug" { 0 } else { 2 };
        assert_eq!(
            cpp.matches("__builtin_unreachable()").count(),
            assumed,
            "{mode}: `or unreachable` lowering\n{cpp}"
        );
        assert!(cpp.contains("::dz::unreachable("), "{mode}: safe `or unreachable`\n{cpp}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// What C++ leaves undefined is defined, in every build (spec §5.1, §10.6, §11; D-0094 point
/// 5, T-0129): `MIN / -1` wraps and `MIN % -1` is 0, a shift count is taken modulo the width
/// and a negative value shifted left wraps, a float converted to an integer saturates and
/// NaN is 0. The operands come from calls, so no compiler can fold them away. A literal
/// divisor or count keeps C++'s own operator, and a division by zero panics, naming its line.
#[test]
fn arithmetic_cpp_leaves_undefined_is_defined() {
    if !have_compiler() {
        eprintln!("skipping: no C++ compiler available");
        return;
    }
    let dir = scratch(
        "dz_defined_arith",
        "[module]\nname = \"arith\"\nsources = [\"src/**/*.dz\"]\n",
        "fn minus_one() -> i32 {\n    return -1\n}\n\nfn forty() -> i32 {\n    return 40\n}\n\nfn huge() -> f64 {\n    return 1e20\n}\n\nfn nan() -> f64 {\n    return 0.0 / 0.0\n}\n\nfn zero() -> i32 {\n    return 0\n}\n\nfn main() {\n    let min: i32 = -2147483648\n    let m8: i8 = -128\n    let neg: i8 = -1\n    println!(\"{} {} {} {}\", min / minus_one(), min % minus_one(), m8 / neg, m8 % neg)\n    println!(\"{} {} {} {}\", 1 << forty(), 1u8 << 9u8, -1 << 3, min << 1)\n    println!(\"{} {} {}\", huge() as i32, (-huge()) as i32, nan() as i32)\n    println!(\"{} {} {}\", 300.7 as u8, (-5.5) as u8, 7 / 2)\n    let mut x = 100\n    x /= minus_one()\n    x %= 7\n    println!(\"{}\", x)\n    println!(\"{}\", 10 / zero())\n}\n",
    );
    for mode in ["--debug", "--release"] {
        let out = Command::new(dz())
            .args(["run", dir.to_str().unwrap(), mode])
            .output()
            .expect("failed to run dz");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success(), "{mode}: expected a panic\n{stderr}");
        assert_eq!(
            normalise(&String::from_utf8_lossy(&out.stdout)),
            "-2147483648 0 -128 0\n256 2 -8 0\n2147483647 -2147483648 0\n255 0 3\n-2",
            "{mode}"
        );
        assert!(stderr.contains("division by zero"), "{mode}: {stderr}");
        assert!(stderr.contains("main.dz:33:"), "{mode}: the panic names its line\n{stderr}");
        let cpp = std::fs::read_to_string(dir.join("build/cpp/arith.cpp")).unwrap();
        assert!(cpp.contains("::dz::num::div<int32_t>("), "{mode}\n{cpp}");
        assert!(cpp.contains("::dz::num::div_assign<int32_t>("), "{mode}\n{cpp}");
        assert!(cpp.contains("::dz::num::float_to_int<"), "{mode}\n{cpp}");
        assert!(cpp.contains("(7 / 2)"), "a literal divisor keeps C++'s `/`\n{cpp}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A safe `or unreachable` that is reached panics in a release build too, naming the line,
/// where it used to let the optimiser run a search past the end of its list (D-0094 point 4,
/// T-0128).
#[test]
fn unreachable_reached_panics_in_release() {
    if !have_compiler() {
        eprintln!("skipping: no C++ compiler available");
        return;
    }
    let dir = scratch(
        "dz_unreachable_release",
        "[module]\nname = \"unreached\"\nsources = [\"src/**/*.dz\"]\n\n[dependencies]\nstd = { open = true }\n",
        "fn find(xs: &List<i32>, v: i32) -> usize? {\n    for i in 0..xs.len() {\n        if xs[i] == v {\n            return i\n        }\n    }\n    return none\n}\n\nfn main() {\n    let mut xs = List<i32>()\n    xs.push(1)\n    let i = find(&xs, 99) or unreachable\n    println!(\"found {}\", i)\n}\n",
    );
    for mode in ["--release", "--shipping"] {
        let out = Command::new(dz())
            .args(["run", dir.to_str().unwrap(), mode])
            .output()
            .expect("failed to run dz");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success(), "{mode}: expected a panic\n{stderr}");
        assert!(
            stderr.contains("reached code the compiler proved unreachable"),
            "{mode}: {stderr}"
        );
        assert!(!String::from_utf8_lossy(&out.stdout).contains("found"), "{mode}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A formatting macro writes each argument with a direct `Format::format` call, through no
/// vtable — a pointer's through what it points at — and an argument that looks into a
/// temporary is written before the temporary is dropped (spec §12.9, D-0080). The first
/// is checked in the generated C++, where only the program's own file is looked at: `std`
/// still declares the trait object type.
#[test]
fn formatting_dispatches_statically_and_keeps_temporaries() {
    if !have_compiler() {
        eprintln!("skipping: no C++ compiler available");
        return;
    }
    let dir = scratch(
        "dz_static_format",
        "[module]\nname = \"statics\"\nsources = [\"src/**/*.dz\"]\n\n[dependencies]\nstd = { open = true }\n",
        "struct Loud {\n    pub label: String\n\n    pub fn text(&self) -> &str {\n        return self.label.as_str()\n    }\n}\n\nimpl Drop for Loud {\n    fn drop(&mut self) {\n        println!(\"drop {}\", self.label)\n    }\n}\n\nfn make(s: &str) -> Loud {\n    return Loud{ label: String(s) }\n}\n\nfn main() {\n    let n = 7\n    let p: ^i32 = &n\n    println!(\"{} {} {}\", make(\"a\").text(), make(\"b\").text(), p)\n}\n",
    );
    let out =
        Command::new(dz()).args(["run", dir.to_str().unwrap()]).output().expect("failed to run dz");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let stdout = normalise(&String::from_utf8_lossy(&out.stdout));
    // Which temporary C++ drops first is its business; that both outlive the line is not.
    // `make`'s moved-from `Loud`s are dropped too, reset to an empty label (§6.4).
    let mut lines: Vec<&str> = stdout.lines().filter(|l| l.trim_end() != "drop").collect();
    assert_eq!(lines.first(), Some(&"a b 7"), "{stdout}");
    lines.sort_unstable();
    assert_eq!(lines, ["a b 7", "drop a", "drop b"], "{stdout}");
    let cpp = std::fs::read_to_string(dir.join("build/cpp/statics.cpp")).unwrap();
    assert!(!cpp.contains("Format__dyn"), "a formatting call went through a vtable:\n{cpp}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// `#[inline]` puts a body in its module's header, where another module's call can be
/// inlined, and not in the module's own file (spec §12.8, T-0153); so does an associated
/// constant, which needs no attribute: `Allocators::Global` runs on every move of a
/// `String`. Pinned on the C++, since the program prints the same either way.
#[test]
fn an_inline_body_is_in_the_header() {
    let dir = scratch(
        "dz_inline_header",
        "[module]\nname = \"inl\"\nsources = [\"src/**/*.dz\"]\n\n[dependencies]\nstd = { open = true }\n",
        "#[inline]\nfn twice(x: i32) -> i32 {\n    return x * 2\n}\n\nfn thrice(x: i32) -> i32 {\n    return x * 3\n}\n\nstruct Box2 {\n    pub const SIDE: i32 = 2\n\n    #[inline]\n    pub fn area(&self) -> i32 {\n        return 4\n    }\n}\n\nfn main() {\n    println!(\"{} {} {}\", twice(1), thrice(1), Box2{}.area())\n}\n",
    );
    let out =
        Command::new(dz()).args(["cpp", dir.to_str().unwrap()]).output().expect("failed to run dz");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let header = std::fs::read_to_string(dir.join("build/cpp/inl.h")).unwrap();
    let source = std::fs::read_to_string(dir.join("build/cpp/inl.cpp")).unwrap();
    assert!(header.contains("inline int32_t twice(int32_t x) {"), "{header}");
    assert!(header.contains("inline int32_t Box2::area() const {"), "{header}");
    assert!(!source.contains("twice(int32_t x) {"), "{source}");
    assert!(!source.contains("Box2::area() const {"), "{source}");
    // Without the attribute, the body stays in the module's own file.
    assert!(source.contains("int32_t thrice(int32_t x) {"), "{source}");
    let std_header = std::fs::read_to_string(dir.join("build/cpp/std.h")).unwrap();
    assert!(std_header.contains("inline Allocator__dyn Allocators::Global() {"), "{std_header}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A field of a temporary is not a place (spec §12.9). When an earlier argument makes a
/// formatting call hoist its arguments into variables, `v.lerp(w, t).x` has to be hoisted
/// as its value: hoisted as an address it pointed into a `Vec3` that died with the
/// statement, which `tests/run/math` printed as garbage under g++ with optimisation on
/// Linux only. The emitted C++ is checked, since undefined behaviour need not show.
#[test]
fn a_field_of_a_temporary_is_hoisted_as_a_value() {
    if !have_compiler() {
        eprintln!("skipping: no C++ compiler available");
        return;
    }
    let dir = scratch(
        "dz_field_of_temporary",
        "[module]\nname = \"hoisted\"\nsources = [\"src/**/*.dz\"]\n\n[dependencies]\nstd = { open = true }\n",
        "fn main() {\n    let x = Vec3{ x: 1, y: 0, z: 0 }\n    let y = Vec3{ x: 0, y: 1, z: 0 }\n    println!(\"{} {} {}\", x.distance(y) * x.distance(y), x.lerp(y, 0.5).x, x == y)\n}\n",
    );
    let out = Command::new(dz())
        .args(["run", "--release", dir.to_str().unwrap()])
        .output()
        .expect("failed to run dz");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(normalise(&String::from_utf8_lossy(&out.stdout)), "2 0.5 false");
    let cpp = std::fs::read_to_string(dir.join("build/cpp/hoisted.cpp")).unwrap();
    let hoisted = cpp.lines().find(|l| l.contains("lerp(y, 0.5f)")).expect("the lerp call");
    // Hoisted it is — the arguments before `x == y` are — but as the `float`, not a pointer.
    assert!(hoisted.trim_start().starts_with("float _dz_arg"), "hoisted by address:\n{hoisted}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// `Process::args`, `Process::env` and `Process::exit` (spec §14.4), on the built binary run
/// directly: a non-ASCII argument arrives as UTF-8 (on Windows it is re-read from the wide
/// command line), a variable set for the child is seen, and the exit code is the program's.
#[test]
fn process_arguments_environment_and_exit_code() {
    if !have_compiler() {
        eprintln!("skipping: no C++ compiler available");
        return;
    }
    let dir = copy_fixture("process", "dz_process_args");
    let built = Command::new(dz()).args(["build", dir.to_str().unwrap()]).output().expect("run dz");
    assert!(built.status.success(), "{}", String::from_utf8_lossy(&built.stderr));
    let exe = dir.join("build/bin").join(if cfg!(windows) { "process.exe" } else { "process" });
    let out = Command::new(&exe)
        .args(["héllo wörld", "two", "7"])
        .env("DZ_PROCESS_GREETING", "✓ hi")
        .output()
        .expect("run the program");
    assert_eq!(out.status.code(), Some(7), "{}", String::from_utf8_lossy(&out.stderr));
    let stdout = normalise(&String::from_utf8_lossy(&out.stdout));
    assert_eq!(
        stdout,
        "program named true\narguments 3\narg 1 héllo wörld\narg 2 two\narg 3 7\nPATH set true\n\
         unset true\ngreeting ✓ hi\nexiting with 7"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A module may be named after anything the runtime declares in `namespace dz` — `panic`,
/// `str`, `index` — since C++ refuses a namespace and a function of one name in one scope.
/// A module called `overflow` once failed to compile against the shim's `dz::overflow`.
#[test]
fn a_module_may_share_a_name_with_the_runtime() {
    if !have_compiler() {
        eprintln!("skipping: no C++ compiler available");
        return;
    }
    let dir = scratch(
        "dz_runtime_names",
        "[module]\nname = \"panic\"\nsources = [\"src/**/*.dz\"]\n\n[dependencies]\nstr = { path = \"str\" }\n",
        "fn main() {\n    println!(\"{}\", str::index(4))\n}\n",
    );
    std::fs::create_dir_all(dir.join("str/src")).unwrap();
    std::fs::write(
        dir.join("str/dazzle.toml"),
        "[module]\nname = \"str\"\nsources = [\"src/**/*.dz\"]\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("str/src/lib.dz"),
        "pub fn index(n: i32) -> i32 {\n    return n * 2\n}\n",
    )
    .unwrap();
    let out = Command::new(dz()).args(["run", dir.to_str().unwrap()]).output().expect("run dz");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(normalise(&String::from_utf8_lossy(&out.stdout)), "8");
    let _ = std::fs::remove_dir_all(&dir);
}

/// `eprint`/`eprintln` write to stderr and `print`/`println` to stdout, each formatting as
/// the other does (spec §12.9). A fixture's `expected.txt` sees only stdout, so this is
/// asserted on the two streams directly.
#[test]
fn eprintln_writes_to_stderr() {
    if !have_compiler() {
        eprintln!("skipping: no C++ compiler available");
        return;
    }
    let dir = scratch(
        "dz_stderr",
        "[module]\nname = \"streams\"\nsources = [\"src/**/*.dz\"]\n",
        "fn main() {\n    println!(\"out {}\", 1)\n    eprintln!(\"err {:x}\", 255)\n    print!(\"a\")\n    eprint!(\"b{{}}\")\n    println!(\"\")\n}\n",
    );
    let out = Command::new(dz()).args(["run", dir.to_str().unwrap()]).output().expect("run dz");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(normalise(&String::from_utf8_lossy(&out.stdout)), "out 1\na");
    let stderr = normalise(&String::from_utf8_lossy(&out.stderr));
    assert!(stderr.ends_with("err ff\nb{}"), "{stderr}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Memory tracking (spec §14.2), turned on for one build with `--track-memory`: a clean
/// program says nothing more, a leak is reported at exit with the line that allocated it,
/// and a double free or a free of the wrong size stops the program at the free.
#[test]
fn a_tracked_build_reports_leaks_and_bad_frees() {
    if !have_compiler() {
        eprintln!("skipping: no C++ compiler available");
        return;
    }
    let run = |dir: &Path| {
        Command::new(dz())
            .args(["run", dir.to_str().unwrap(), "--track-memory"])
            .output()
            .expect("failed to run dz")
    };

    // The containers too: a tracked build of a program that frees everything prints exactly
    // what an untracked one does, so any leak report would break the comparison.
    for fixture in [
        "allocators",
        "hashmap",
        "handles",
        "files",
        "handle_upcast",
        "sorting",
        "collections",
        "region_arena",
    ] {
        let clean = copy_fixture(fixture, &format!("dz_tracked_clean_{fixture}"));
        let out = run(&clean);
        let expected =
            normalise(&std::fs::read_to_string(run_dir(fixture).join("expected.txt")).unwrap());
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(normalise(&String::from_utf8_lossy(&out.stdout)), expected, "{fixture}");
        let _ = std::fs::remove_dir_all(&clean);
    }

    let manifest = "[module]\nname = \"tracked\"\nsources = [\"src/**/*.dz\"]\n\n[dependencies]\nstd = { open = true }\n";
    let leak = scratch(
        "dz_tracked_leak",
        manifest,
        "fn main() {\n    let g = Allocators::Global\n    let kept = g.alloc(24, 8)\n    let _ = kept\n}\n",
    );
    let out = run(&leak);
    // The report goes to stderr, so a leaking program's stdout is still only its own.
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stderr}");
    assert!(out.stdout.is_empty(), "{}", String::from_utf8_lossy(&out.stdout));
    assert!(stderr.contains("leak: 24 bytes allocated at"), "{stderr}");
    assert!(stderr.contains("main.dz:3"), "the report names the allocating line:\n{stderr}");
    let _ = std::fs::remove_dir_all(&leak);

    for (name, body, message) in [
        ("double", "    g.free(p, 8, 8)\n    g.free(p, 8, 8)\n", "double free"),
        ("size", "    g.free(p, 16, 8)\n", "free of 16 bytes"),
    ] {
        let dir = scratch(
            &format!("dz_tracked_{name}"),
            manifest,
            &format!("fn main() {{\n    let g = Allocators::Global\n    let p = g.alloc(8, 8)\n{body}}}\n"),
        );
        let out = run(&dir);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success(), "{name}: expected a panic");
        assert!(stderr.contains(message), "{name}: {stderr}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// A tracked build overwrites what it frees with `0xDD` (221), so a read through a dangling
/// reference sees garbage rather than the old value (T-0131): a small block freed in a
/// region, which stays in its chunk; a region's chunk once the region ends, which goes back
/// to the pool; and a block freed in `Allocators::Temp`. All three are still the program's
/// memory, so reading them is the same on every host; an untracked build reads the old
/// bytes. The heap's own `free` poisons too, but what C's heap does with a freed block
/// differs by platform, so it is not read here.
#[test]
fn a_tracked_build_poisons_what_it_frees() {
    if !have_compiler() {
        eprintln!("skipping: no C++ compiler available");
        return;
    }
    let manifest = "[module]\nname = \"poison\"\nsources = [\"src/**/*.dz\"]\n\n[dependencies]\nstd = { open = true }\n";
    let source = "\
fn main() {
    let mut kept: usize = 0
    {
        let mut arena = ScopeArena{}
        let freed = arena.alloc(16, 8)
        let after = arena.alloc(16, 8)
        unsafe {
            freed[0] = 7
            after[0] = 8
        }
        arena.free(freed, 16, 8)
        println!(\"region free {}\", unsafe { freed[0] })
        kept = after as usize
    }
    println!(\"region end {}\", unsafe { (kept as ^mut u8)[0] })
    let t = Allocators::Temp
    let a = t.alloc(16, 8)
    let b = t.alloc(16, 8)
    unsafe {
        a[0] = 9
        b[0] = 10
    }
    t.free(a, 16, 8)
    println!(\"temp free {}\", unsafe { a[0] })
}
";
    for (tracked, expected) in [
        (true, "region free 221\nregion end 221\ntemp free 221"),
        (false, "region free 7\nregion end 8\ntemp free 9"),
    ] {
        let dir = scratch(&format!("dz_poison_{tracked}"), manifest, source);
        let mut args = vec!["run", dir.to_str().unwrap()];
        if tracked {
            args.push("--track-memory");
        }
        let out = Command::new(dz()).args(&args).output().expect("failed to run dz");
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(normalise(&String::from_utf8_lossy(&out.stdout)), expected, "tracked {tracked}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// `--release` overrides the manifest's `[build] optimize` for one command.
///
/// Asserted through behaviour rather than through the command line the driver built: the
/// point is that the program still compiles and runs identically at every level, which is
/// what makes the flag safe to reach for when measuring.
#[test]
fn a_build_mode_can_be_chosen_on_the_command_line() {
    if !have_compiler() {
        eprintln!("skipping: no C++ compiler available");
        return;
    }
    // A scratch copy, not `tests/run/hello` itself: cargo runs tests in parallel and
    // `hello_world_runs` builds that directory too, so sharing one would race in `build/`.
    let dir = scratch(
        "dz_build_modes",
        "[module]\nname = \"hello\"\nsources = [\"src/**/*.dz\"]\n\n[dependencies]\nstd = { open = true }\n",
        "fn main() {\n    println!(\"Hello, world!\")\n}\n",
    );
    for mode in ["--debug", "--release", "--shipping"] {
        let out = Command::new(dz())
            .args(["run", dir.to_str().unwrap(), mode])
            .output()
            .expect("failed to run dz");
        assert!(
            out.status.success(),
            "{mode}: dz run failed\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(
            normalise(&String::from_utf8_lossy(&out.stdout)),
            "Hello, world!",
            "{mode}: stdout differs"
        );
    }

    let bad = Command::new(dz())
        .args(["run", dir.to_str().unwrap(), "--nonsense"])
        .output()
        .expect("failed to run dz");
    assert!(!bad.status.success(), "an unknown option should be refused");
}

/// `dz test` finds `#[test]` functions, runs each one, and reports (spec §19 M14).
///
/// Also asserts the half that is easy to lose: a **normal build leaves the tests out**.
/// `every_program_produces_its_expected_output` covers the other side of it — running the
/// same project prints only what `main` prints.
#[test]
fn dz_test_runs_the_projects_tests() {
    if !have_compiler() {
        eprintln!("skipping: no C++ compiler available");
        return;
    }
    let dir = copy_fixture("tests", "dz_test_fixture");

    let out = Command::new(dz())
        .args(["test", dir.to_str().unwrap()])
        .output()
        .expect("failed to run dz");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "dz test failed\n{stdout}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(stdout.contains("running 3 tests"), "{stdout}");
    assert!(stdout.contains("ok    tests::addition_works"), "{stdout}");
    assert!(stdout.contains("3 passed"), "{stdout}");

    // A filter selects by substring.
    let filtered = Command::new(dz())
        .args(["test", dir.to_str().unwrap(), "commutes"])
        .output()
        .expect("failed to run dz");
    let text = String::from_utf8_lossy(&filtered.stdout);
    assert!(text.contains("running 1 test"), "{text}");
    assert!(text.contains("addition_commutes"), "{text}");

    // The program build must not carry them: `addition_works` exists only in the test
    // binary, and linking tests into a shipped program is exactly the mistake to catch.
    let cpp =
        Command::new(dz()).args(["cpp", dir.to_str().unwrap()]).output().expect("failed to run dz");
    assert!(cpp.status.success(), "{}", String::from_utf8_lossy(&cpp.stderr));
    let generated = std::fs::read_to_string(dir.join("build/cpp/tests.cpp")).unwrap();
    assert!(
        !generated.contains("addition_works"),
        "a normal build must not contain the tests:\n{generated}"
    );
}

/// Debug information names real files (spec §16.3): the standard library, which is compiled
/// into `dz`, is written into the shared cache, and every `#line` naming it points there —
/// indented ones inside generated lambdas included — so a debugger can open what it steps
/// into. A panic's location still reads `<std>/collections/list.dz`.
#[test]
fn line_directives_name_files_a_debugger_can_open() {
    let dir = scratch(
        "dz_debug_lines",
        "[module]\nname = \"lines\"\nsources = [\"src/**/*.dz\"]\n\n[dependencies]\nstd = { open = true }\n",
        "fn main() {\n    let mut xs = List<i32>()\n    xs.push(1)\n    println!(\"{}\", xs.len())\n}\n",
    );
    // The copies are in the cache every project shares (T-0182), which this test puts in
    // its own folder.
    let cache = dir.join("cache");
    let out = Command::new(dz())
        .args(["cpp", dir.to_str().unwrap()])
        .env("DZ_CACHE_DIR", &cache)
        .output()
        .expect("failed to run dz");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let mut named = Vec::new();
    for entry in std::fs::read_dir(dir.join("build/cpp")).unwrap().flatten() {
        let Ok(text) = std::fs::read_to_string(entry.path()) else { continue };
        for line in text.lines().filter(|l| l.trim_start().starts_with("#line ")) {
            assert!(!line.contains("<std>"), "{}: {line}", entry.path().display());
            if line.ends_with("/std/collections/list.dz\"") {
                named.push(line.rsplit('"').nth(1).unwrap().to_owned());
            }
        }
    }
    assert!(!named.is_empty(), "the standard library's own code carries `#line`s naming the copy");
    let copy = PathBuf::from(&named[0]);
    assert!(copy.starts_with(&cache), "{} is not in the cache", copy.display());
    assert!(named.iter().all(|n| *n == named[0]), "one copy: {named:?}");
    assert_eq!(
        std::fs::read_to_string(&copy).expect("the standard library is written out"),
        dz_project::std_source(Path::new("<std>/collections/list.dz")).unwrap()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A `#line` names its source by its full path, however the project was named: by a
/// relative path or by its full one. So a debugger started from another directory finds it,
/// and the directives do not change with how the project was spelled (T-0012 found it).
/// A panic's location still says the path as it was given, which is the program's output.
#[test]
fn line_directives_name_sources_by_their_full_path() {
    let dir = scratch(
        "dz_named_paths",
        "[module]\nname = \"named\"\nsources = [\"src/**/*.dz\"]\n\n[dependencies]\nstd = { open = true }\n",
        "fn main() {\n    let mut xs = List<i32>()\n    xs.push(1)\n    println!(\"{}\", xs.len())\n}\n",
    );
    // As the current directory reports it: macOS reaches its temporary directory through a
    // link. (Not on Windows, where this adds a `\\?\` prefix no current directory has.)
    let dir = if cfg!(windows) { dir } else { std::fs::canonicalize(&dir).unwrap() };
    let cache = dir.join("cache");
    let generate = |cwd: &Path, path: &str| {
        let out = Command::new(dz())
            .current_dir(cwd)
            .args(["cpp", path])
            .env("DZ_CACHE_DIR", &cache)
            .output()
            .expect("failed to run dz");
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        let text = std::fs::read_to_string(dir.join("build/cpp/named.cpp")).unwrap();
        text.lines().filter(|l| l.starts_with("#line ")).collect::<Vec<_>>().join("\n")
    };
    let relative = generate(dir.parent().unwrap(), "dz_named_paths");
    let absolute = generate(Path::new("/"), dir.to_str().unwrap());
    let full = dir.display().to_string().replace('\\', "/");
    assert!(relative.contains(&format!("\"{full}")), "{relative}");
    assert_eq!(relative, absolute);
    let _ = std::fs::remove_dir_all(&dir);
}

/// `destroy` of a number, a pointer or an enum ends no lifetime (T-0189). `String` stores
/// its capacity into a word and then `destroy`s that word, as a signal to the ties check
/// (spec §6.7). As a C++ pseudo-destructor that ended the word's lifetime, and g++ at -O2
/// deleted the store before it as dead: once it inlined the two together, a growing string
/// kept its old capacity and the next append wrote past its end. This is that pattern,
/// against the runtime itself, with every compiler there is, optimised.
#[test]
fn destroying_a_word_keeps_what_was_stored_in_it() {
    let dir = std::env::temp_dir().join(format!("dz_destroy_word_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("dz_runtime.h"), dz_cpp::RUNTIME_H).unwrap();
    std::fs::write(
        dir.join("word.cpp"),
        "#include \"dz_runtime.h\"\n#include <stdio.h>\n\
         struct S { size_t words[2]; };\n\
         __attribute__((noinline)) void grow(S& s, size_t room) {\n\
         \x20   s.words[1] = room;\n    ::dz::destroy(s.words[1]);\n}\n\
         int main() {\n    S s{{0, 7}};\n    grow(s, 41);\n    printf(\"%zu\\n\", s.words[1]);\n\
         \x20   return 0;\n}\n",
    )
    .unwrap();
    let mut tried = 0;
    for cxx in ["clang++", "g++"] {
        let exe = dir.join(format!("word-{cxx}"));
        let Ok(out) = Command::new(cxx)
            .args(["-std=c++17", "-O2", "-Wall", "-Wextra", "-fno-exceptions", "-fno-rtti"])
            .arg(dir.join("word.cpp"))
            .arg("-o")
            .arg(&exe)
            .output()
        else {
            continue;
        };
        tried += 1;
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success() && stderr.trim().is_empty(), "{cxx}: {stderr}");
        let run = Command::new(&exe).output().unwrap();
        assert_eq!(String::from_utf8_lossy(&run.stdout).trim(), "41", "with {cxx}");
    }
    if tried == 0 {
        eprintln!("skipping: neither clang++ nor g++ is installed");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// The runtime and `std` are compiled once for every project (T-0182): a second project,
/// built from clean, links the objects the first one compiled, and compiles only its own.
#[test]
fn a_second_project_reuses_the_compiled_standard_library() {
    if !have_compiler() {
        eprintln!("skipping: no C++ compiler available");
        return;
    }
    let manifest = |name: &str| {
        format!(
            "[module]\nname = \"{name}\"\nsources = [\"src/**/*.dz\"]\n\n\
             [dependencies]\nstd = {{ open = true }}\n"
        )
    };
    let first =
        scratch("dz_shared_first", &manifest("first"), "fn main() {\n    println!(\"one\")\n}\n");
    let second =
        scratch("dz_shared_second", &manifest("second"), "fn main() {\n    println!(\"two\")\n}\n");
    let cache = first.join("cache");
    let build = |dir: &Path| {
        let out = Command::new(dz())
            .args(["run", dir.to_str().unwrap()])
            .env("DZ_CACHE_DIR", &cache)
            .output()
            .expect("failed to run dz");
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        // stdout is in text mode on Windows, so the program's `\n` arrives as `\r\n`.
        String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n")
    };
    // Every object in `dir`, with when it was written.
    let objects = |dir: &Path| -> Vec<(PathBuf, std::time::SystemTime)> {
        let mut found = Vec::new();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|x| x == "o") {
                    found.push((path.clone(), entry.metadata().unwrap().modified().unwrap()));
                }
            }
        }
        found.sort();
        found
    };
    let named = |objects: &[(PathBuf, std::time::SystemTime)], stem: &str| {
        objects
            .iter()
            .filter(|(p, _)| p.file_name().unwrap().to_string_lossy().starts_with(stem))
            .count()
    };

    assert_eq!(build(&first), "one\n");
    let shared = objects(&cache);
    assert_eq!(named(&shared, "std-"), 1, "{shared:?}");
    assert_eq!(named(&objects(&first.join("build")), "std-"), 0);

    assert_eq!(build(&second), "two\n");
    assert_eq!(objects(&cache), shared, "the shared objects were compiled again");
    let own = objects(&second.join("build"));
    assert_eq!(named(&own, "second-"), 1, "{own:?}");
    for stem in ["std-", "dz_runtime-", "dz_native-"] {
        assert_eq!(named(&own, stem), 0, "{own:?}");
    }
    let _ = std::fs::remove_dir_all(&first);
    let _ = std::fs::remove_dir_all(&second);
}

/// Where a release build cannot link with `-flto=auto`, it is built again without, and that
/// is remembered: the next build, even of a project built from clean, does not try (T-0183).
/// A link that fails for want of a definition fails without it too, and is not remembered.
/// The compiler is a script around the real one that fails a link with LTO, as clang++
/// targeting MSVC does without `lld`.
#[cfg(unix)]
#[test]
fn a_toolchain_that_cannot_link_with_lto_is_remembered() {
    use std::os::unix::fs::PermissionsExt;
    if !have_compiler() {
        eprintln!("skipping: no C++ compiler available");
        return;
    }
    let dir = scratch(
        "dz_no_lto",
        "[module]\nname = \"nolto\"\nsources = [\"src/**/*.dz\"]\n\n[dependencies]\nstd = { open = true }\n\n[build]\noptimize = \"release\"\n",
        "fn main() {\n    println!(\"linked\")\n}\n",
    );
    let real = std::env::var("DZ_CXX").ok().filter(|c| !c.is_empty());
    let real = real.unwrap_or_else(|| {
        let found = ["clang++", "g++", "c++"].into_iter().find(
            |c| matches!(Command::new(c).arg("--version").output(), Ok(o) if o.status.success()),
        );
        found.unwrap().to_owned()
    });
    let script = dir.join("cxx.sh");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nlto=0; link=1\n\
             for a in \"$@\"; do case \"$a\" in -flto=auto) lto=1;; -c|-x|--version) link=0;; esac; done\n\
             [ $lto = 1 ] && echo lto >> \"{log}\"\n\
             if [ $link = 1 ] && [ -n \"$MISSING\" ]; then echo 'undefined reference to `f'\"'\" >&2; exit 1; fi\n\
             if [ $link = 1 ] && [ $lto = 1 ]; then echo 'error: LTO requires -fuse-ld=lld' >&2; exit 1; fi\n\
             exec {real} \"$@\"\n",
            log = dir.join("lto.log").display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let cache = dir.join("cache");
    let build = |missing: bool| {
        let _ = std::fs::remove_dir_all(dir.join("build"));
        let _ = std::fs::remove_file(dir.join("lto.log"));
        let mut cmd = Command::new(dz());
        cmd.args(["run", "--cxx", script.to_str().unwrap(), dir.to_str().unwrap()]);
        cmd.env("DZ_CACHE_DIR", &cache);
        if missing {
            cmd.env("MISSING", "1");
        }
        let out = cmd.output().expect("failed to run dz");
        let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
        let tried = std::fs::read_to_string(dir.join("lto.log")).unwrap_or_default();
        (out.status.success(), stderr, tried.lines().count())
    };

    // A missing definition: a failure, and LTO is tried again next time.
    let (ok, stderr, _) = build(true);
    assert!(!ok && stderr.contains("could not find"), "{stderr}");
    let (ok, stderr, tried) = build(false);
    assert!(ok, "{stderr}");
    assert!(tried > 0, "LTO was not tried again after a missing definition");
    assert!(stderr.contains("could not link with `-flto=auto`"), "{stderr}");
    // Remembered: a build from clean compiles nothing with it.
    let (ok, stderr, tried) = build(false);
    assert!(ok, "{stderr}");
    assert_eq!(tried, 0, "LTO was tried again: {stderr}");
    assert!(stderr.contains("to try again"), "{stderr}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A test that panics is reported as a failure, by name, with its own message — and the
/// tests after it still run, which is the reason each one gets its own process.
#[test]
fn a_failing_test_is_reported_and_does_not_stop_the_rest() {
    if !have_compiler() {
        eprintln!("skipping: no C++ compiler available");
        return;
    }
    let source = "#[test]\nfn passes() {}\n\n\
                  #[test]\nfn fails() {\n    panic!(\"deliberately broken\")\n}\n\n\
                  #[test]\nfn also_passes() {}\n\nfn main() {}\n";
    let dir = scratch(
        "dz_test_failing",
        "[module]\nname = \"failing\"\nsources = [\"src/**/*.dz\"]\n\n[dependencies]\nstd = { open = true }\n",
        source,
    );
    let out = Command::new(dz())
        .args(["test", dir.to_str().unwrap()])
        .output()
        .expect("failed to run dz");
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert!(!out.status.success(), "a failing test must fail the run\n{stdout}");
    assert!(stdout.contains("FAIL  failing::fails"), "{stdout}");
    assert!(stdout.contains("deliberately broken"), "the panic message is the report\n{stdout}");
    // The point of one process per test: what follows a failure still runs.
    assert!(stdout.contains("ok    failing::also_passes"), "{stdout}");
    assert!(stdout.contains("2 passed, 1 failed"), "{stdout}");
}

/// `dz bench` times `#[bench]` functions and reports ns/iter (spec §19 M15).
///
/// The comparison suite itself is not asserted on — a number from one machine is not a
/// fact about the language — but the machinery is: benchmarks are found, timed, and
/// excluded from the program, and the two halves of each pair agree on what they compute,
/// which is what makes comparing them meaningful at all.
#[test]
fn dz_bench_times_the_projects_benchmarks() {
    if !have_compiler() {
        eprintln!("skipping: no C++ compiler available");
        return;
    }
    let source = "fn work(n: i32) -> i32 {\n    let mut t = 0\n    let mut i = 0\n    \
                  while i < n {\n        t += i\n        i += 1\n    }\n    return t\n}\n\n\
                  #[bench]\nfn adding() {\n    blackbox(work(blackbox(64)))\n}\n\n\
                  fn main() {\n    println!(\"{}\", work(4))\n}\n";
    let dir =
        scratch("dz_bench_smoke", "[module]\nname = \"b\"\nsources = [\"src/**/*.dz\"]\n\n[dependencies]\nstd = { open = true }\n", source);
    let out = Command::new(dz())
        .args(["bench", dir.to_str().unwrap()])
        .output()
        .expect("failed to run dz");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "dz bench failed\n{stdout}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(stdout.contains("running 1 benchmark"), "{stdout}");
    assert!(stdout.contains("b::adding"), "{stdout}");
    assert!(stdout.contains("ns/iter"), "a benchmark reports a time\n{stdout}");

    // The program build carries neither the benchmark nor `blackbox`'s work.
    let cpp =
        Command::new(dz()).args(["cpp", dir.to_str().unwrap()]).output().expect("failed to run dz");
    assert!(cpp.status.success(), "{}", String::from_utf8_lossy(&cpp.stderr));
    let generated = std::fs::read_to_string(dir.join("build/cpp/b.cpp")).unwrap();
    assert!(!generated.contains("adding"), "a normal build must not contain benchmarks");
}

/// The benchmark suite's two halves must compute the same thing, or the comparison between
/// them measures nothing (spec §19 M15). Running the project is what checks that: `main`
/// prints each Dazzle result beside the C++ one it is measured against.
#[test]
fn the_benchmark_suite_agrees_with_its_cpp_baseline() {
    if !have_compiler() {
        eprintln!("skipping: no C++ compiler available");
        return;
    }
    let dir = repo_root().join("tests/bench");
    let out =
        Command::new(dz()).args(["run", dir.to_str().unwrap()]).output().expect("failed to run dz");
    if !out.status.success() {
        // `import cpp` needs libclang; without it the suite cannot be built at all.
        eprintln!("skipping: the benchmark suite did not build (libclang missing?)");
        return;
    }
    for line in normalise(&String::from_utf8_lossy(&out.stdout)).lines() {
        let mut halves = line.split_whitespace();
        let (dazzle, cpp) = (halves.next(), halves.next());
        assert!(
            dazzle.is_some() && dazzle == cpp,
            "a benchmark pair computes different things: {line:?}"
        );
    }
}

/// `--timings` names every phase a build ran, each with a time, then the total, all on
/// standard error, so a program's own output on standard output is unchanged.
/// `tools/bench_programs.py` reads these lines, so the names are pinned here.
#[test]
fn timings_name_every_phase_on_standard_error() {
    if !have_compiler() {
        eprintln!("skipping: no C++ compiler available");
        return;
    }
    let dir = copy_fixture("hello", "dz_timings");
    let out = Command::new(dz())
        .args(["run", "--timings", dir.to_str().unwrap()])
        .output()
        .expect("failed to run dz");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let stderr = String::from_utf8_lossy(&out.stderr).replace("\r\n", "\n");
    let phases: Vec<&str> = stderr
        .lines()
        .skip_while(|l| *l != "timings:")
        .skip(1)
        .map(|l| {
            let l = l.trim();
            assert!(l.ends_with(" ms"), "not a timing: {l:?}");
            l.trim_end_matches(" ms").rsplit_once(' ').unwrap().0.trim()
        })
        .collect();
    let expected = [
        "manifests",
        "C and C++ headers",
        "lex and parse",
        "#if and macros",
        "resolve names",
        "type check",
        "ownership",
        "placement",
        "instantiate generics",
        "emit C++",
        "C and C++ libraries",
        "C++ compiler",
        "link",
        "total",
    ];
    assert_eq!(phases, expected, "{stderr}");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!stdout.contains("timings"), "timings leaked onto standard output: {stdout}");
}

/// The whole-program benchmarks (`tests/bench_programs`, T-0090): each program's Dazzle half
/// and its C++ half must print the same checksum, on the small input in `quick_args.txt`.
/// The timings belong to `tools/bench_programs.py`; this keeps the pairs honest, since a
/// change to either half that makes them compute different things would otherwise only be
/// noticed the next time someone publishes numbers.
#[test]
fn whole_program_benchmarks_agree_with_their_cpp_halves() {
    if !have_compiler() {
        eprintln!("skipping: no C++ compiler available");
        return;
    }
    let cxx = std::env::var("DZ_CXX").ok().filter(|c| !c.is_empty()).unwrap_or_else(|| {
        ["clang++", "g++", "c++"]
            .into_iter()
            .find(|c| matches!(Command::new(c).arg("--version").output(), Ok(o) if o.status.success()))
            .unwrap()
            .to_owned()
    });
    let root = repo_root().join("tests/bench_programs");
    let mut programs: Vec<PathBuf> = std::fs::read_dir(&root)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("main.cpp").exists())
        .collect();
    programs.sort();
    assert!(programs.len() >= 3, "expected the benchmark programs, found {programs:?}");

    let checksum = |out: &std::process::Output, what: &str| -> String {
        assert!(out.status.success(), "{what} failed: {}", String::from_utf8_lossy(&out.stderr));
        let text = String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n");
        let line = text.lines().find(|l| l.starts_with("checksum "));
        line.unwrap_or_else(|| panic!("{what} printed no checksum:\n{text}")).to_owned()
    };
    let scratch = std::env::temp_dir().join("dz_bench_programs");
    std::fs::create_dir_all(&scratch).unwrap();
    for dir in programs {
        let name = dir.file_name().unwrap().to_string_lossy().into_owned();
        let args_text = std::fs::read_to_string(dir.join("quick_args.txt")).unwrap();
        let args: Vec<&str> = args_text.split_whitespace().collect();

        let built = Command::new(dz()).args(["build", dir.to_str().unwrap()]).output().unwrap();
        assert!(built.status.success(), "{name}: {}", String::from_utf8_lossy(&built.stderr));
        let exe = dir.join("build/bin").join(if cfg!(windows) {
            format!("{name}.exe")
        } else {
            name.clone()
        });
        let dazzle = checksum(&Command::new(&exe).args(&args).output().unwrap(), &name);

        let cpp_exe =
            scratch.join(if cfg!(windows) { format!("{name}.exe") } else { name.clone() });
        // Only the result is checked here, so the C++ half gets no more flags than it needs:
        // `-fno-exceptions` breaks the MSVC standard library a Windows clang++ uses unless
        // it is configured for it. The timings, which do use it, are tools/bench_programs.py.
        let compiled = Command::new(&cxx)
            .args(["-std=c++17", "-O1"])
            .arg(dir.join("main.cpp"))
            .arg("-o")
            .arg(&cpp_exe)
            .output()
            .unwrap();
        assert!(
            compiled.status.success(),
            "{name}/main.cpp: {}",
            String::from_utf8_lossy(&compiled.stderr)
        );
        let cpp = checksum(
            &Command::new(&cpp_exe).args(&args).output().unwrap(),
            &format!("{name}/main.cpp"),
        );
        assert_eq!(dazzle, cpp, "{name}: the Dazzle and C++ halves compute different things");
    }
}

/// Which C++ compiler builds the generated code, and who gets the last word (spec §16.5).
///
/// Four places may say. The test pins the *order* rather than any one of them, by naming a
/// compiler that does not exist at the level being tested and checking that the build fails
/// with that name — the only way to prove which one was actually consulted.
#[test]
fn the_cpp_compiler_can_be_chosen_at_four_levels() {
    if !have_compiler() {
        eprintln!("skipping: no C++ compiler available");
        return;
    }
    let src = "fn main() {\n    println!(\"ok\")\n}\n";
    let plain = "[module]\nname = \"cx\"\nsources = [\"src/**/*.dz\"]\n\n[dependencies]\nstd = { open = true }\n";
    let with_cxx = format!("{plain}\n[build]\ncxx = \"dz-not-a-compiler\"\n");

    let ran = |dir: &std::path::Path, args: &[&str]| -> (bool, String) {
        let mut c = Command::new(dz());
        c.arg("build").arg(dir).args(args);
        // `DZ_CXX` sits *above* the manifest in the precedence this test is checking, and
        // CI sets it for the whole job to pin the matrix entry — so leaving it in the
        // environment made the manifest levels unobservable and the test passed only on a
        // machine that happened not to set it.
        c.env_remove("DZ_CXX");
        let out = c.output().expect("failed to run dz");
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        (out.status.success(), text)
    };

    // 3. The manifest names it, and is used when nothing nearer does.
    let dir = scratch("dz_cxx_manifest", &with_cxx, src);
    let (ok, text) = ran(&dir, &[]);
    assert!(!ok && text.contains("dz-not-a-compiler"), "the manifest should decide:\n{text}");

    // 1. `--cxx` beats the manifest.
    let (ok, text) = ran(&dir, &["--cxx", "clang++"]);
    assert!(
        ok || !text.contains("dz-not-a-compiler"),
        "`--cxx` should override the manifest:\n{text}"
    );

    // 2. `[build.target.<os>]` beats the plain `[build]`.
    let host = if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    };
    let targeted = format!("{with_cxx}\n[build.target.{host}]\ncxx = \"dz-also-not-one\"\n");
    let dir = scratch("dz_cxx_target", &targeted, src);
    let (ok, text) = ran(&dir, &[]);
    assert!(
        !ok && text.contains("dz-also-not-one"),
        "the platform section should beat the base table:\n{text}"
    );
}

/// A vendored library's recipe can pass raw flags to the compiler for its own sources
/// (spec §15.1).
///
/// The case that forced this key: raylib's `rglfw.c` includes GLFW's Cocoa backend, so on
/// macOS it must be compiled as Objective-C despite its `.c` name, which no amount of
/// `defines` or `include_dirs` can say. That path cannot be exercised here, so this checks
/// the mechanism instead — a define moved out of `defines` and into `flags` must still
/// reach the compiler, and the library must still behave the same.
#[test]
fn a_recipe_can_pass_flags_to_the_compiler() {
    if !have_compiler() {
        eprintln!("skipping: no C++ compiler available");
        return;
    }
    let dir = copy_fixture("cinterop", "dz_lib_flags");
    let manifest = dir.join("dazzle.toml");
    let original = std::fs::read_to_string(&manifest).unwrap();
    assert!(
        original.contains("defines      = [\"MINI_SCALE=3\"]"),
        "the fixture should still define MINI_SCALE the ordinary way:\n{original}"
    );
    // `mini.c` refuses to compile without it, so if the build succeeds the flag arrived.
    let moved = original
        .replace("defines      = [\"MINI_SCALE=3\"]", "flags        = [\"-DMINI_SCALE=3\"]");
    std::fs::write(&manifest, moved).unwrap();

    let out =
        Command::new(dz()).args(["run", dir.to_str().unwrap()]).output().expect("failed to run dz");
    assert!(
        out.status.success(),
        "a flag from the recipe should reach the compiler\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let expected = normalise(&std::fs::read_to_string(dir.join("expected.txt")).unwrap());
    assert_eq!(normalise(&String::from_utf8_lossy(&out.stdout)), expected);
}

/// A module whose name is one of `dz`'s own build directories still builds and benches
/// (spec §16.1).
///
/// The executable used to sit directly in `build/`, beside the `cpp/`, `obj/`, `test/` and
/// `bench/` directories — so a module named `bench` produced `build/bench`, a *file*, which
/// is exactly the directory `dz bench` then wanted. It failed with "Not a directory".
/// Windows never showed it, because there the file is `bench.exe`; every other platform did.
/// The executable now lives in `build/bin/`, leaving `build/` to directories alone.
#[test]
fn a_module_named_after_a_build_directory_does_not_collide() {
    if !have_compiler() {
        eprintln!("skipping: no C++ compiler available");
        return;
    }
    let src = "#[bench]\nfn measured() {\n    blackbox(1)\n}\n\n\
               #[test]\nfn checked() {}\n\n\
               fn main() {\n    println!(\"ok\")\n}\n";
    // Not `obj`: that name collides with the runtime shim's `dz::obj` helper rather than
    // with a directory, which is a separate bug — see the deferral register.
    for name in ["bench", "test", "cpp", "bin"] {
        let dir = scratch(
            &format!("dz_collide_{name}"),
            &format!("[module]\nname = \"{name}\"\nsources = [\"src/**/*.dz\"]\n\n[dependencies]\nstd = {{ open = true }}\n"),
            src,
        );
        // Build first, so the executable exists where a harness directory would go.
        for args in [
            vec!["build", dir.to_str().unwrap()],
            vec!["bench", dir.to_str().unwrap()],
            vec!["test", dir.to_str().unwrap()],
        ] {
            let out = Command::new(dz()).args(&args).output().expect("failed to run dz");
            assert!(
                out.status.success(),
                "module named `{name}`: `dz {}` failed\n{}{}",
                args[0],
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
        }
    }
}

#[test]
fn hello_world_runs() {
    // The M4 acceptance criterion, called out on its own so a failure names itself.
    if !have_compiler() {
        return;
    }
    // A scratch copy, for the reason `copy_fixture` gives: `tests/run/hello` is also built
    // by `every_program_produces_its_expected_output`, and two `dz run`s sharing one
    // `build/` overwrite each other's objects mid-link.
    let dir = copy_fixture("hello", "dz_hello_world");
    let out =
        Command::new(dz()).args(["run", dir.to_str().unwrap()]).output().expect("failed to run dz");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(normalise(&String::from_utf8_lossy(&out.stdout)), "Hello, world!");
}

#[test]
fn generated_cpp_carries_line_directives_back_to_dazzle() {
    // spec §16.3 — this is what makes breakpoints, stack traces and any C++ error that
    // slips through point at `.dz` source.
    let dir = copy_fixture("hello", "dz_line_directives");
    let out =
        Command::new(dz()).args(["cpp", dir.to_str().unwrap()]).output().expect("failed to run dz");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));

    let cpp =
        std::fs::read_to_string(dir.join("build/cpp/hello.cpp")).expect("generated hello.cpp");
    assert!(cpp.contains("#line "), "no #line directives:\n{cpp}");
    assert!(cpp.contains("main.dz"), "#line does not name the .dz file:\n{cpp}");
    assert!(cpp.contains("namespace dz"), "output is not in the dz namespace:\n{cpp}");
}

#[test]
fn a_build_ignores_cpp_left_behind_by_a_module_that_is_gone() {
    // T-0087 — `build/cpp/` keeps the `.cpp` of a module that has since been renamed or
    // removed. Compiling it fails against a runtime it was not generated for, so a build
    // compiles only what it emitted, and leaves the folder's other files where they are.
    if !have_compiler() {
        return;
    }
    let dir = copy_fixture("hello", "dz_stale_cpp");
    let cpp = dir.join("build/cpp");
    std::fs::create_dir_all(&cpp).unwrap();
    let stale = cpp.join("removed_module.cpp");
    std::fs::write(&stale, "#error this module no longer exists\n").unwrap();
    let out =
        Command::new(dz()).args(["run", dir.to_str().unwrap()]).output().expect("failed to run dz");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(normalise(&String::from_utf8_lossy(&out.stdout)), "Hello, world!");
    assert!(stale.exists(), "a file the build did not write was deleted");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn no_line_directives_leaves_out_the_directives_and_nothing_else() {
    // spec §16.3 — for reading or debugging the generated C++ itself. Every file, the
    // standard library's included, is the default output with its `#line`s taken out.
    let with = copy_fixture("collections", "dz_with_line_directives");
    let without = copy_fixture("collections", "dz_without_line_directives");
    for (dir, extra) in [(&with, None), (&without, Some("--no-line-directives"))] {
        let mut args = vec!["cpp", dir.to_str().unwrap()];
        args.extend(extra);
        let out = Command::new(dz()).args(&args).output().expect("failed to run dz");
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    }
    let mut compared = 0;
    for entry in std::fs::read_dir(without.join("build/cpp")).unwrap() {
        let path = entry.unwrap().path();
        if !path.extension().is_some_and(|e| e == "cpp" || e == "h") {
            continue;
        }
        let plain = std::fs::read_to_string(&path).unwrap();
        assert!(!plain.contains("#line "), "{} has a #line:\n{plain}", path.display());
        let directed =
            std::fs::read_to_string(with.join("build/cpp").join(path.file_name().unwrap()))
                .unwrap()
                .lines()
                .filter(|l| !l.trim_start().starts_with("#line "))
                .map(|l| format!("{l}\n"))
                .collect::<String>()
                // Source locations name the scratch copy each was built from.
                .replace("dz_with_line_directives", "dz_without_line_directives");
        assert_eq!(plain, directed, "{} differs by more than its #lines", path.display());
        compared += 1;
    }
    assert!(compared > 2, "nothing generated to compare");
    // Nothing names the standard library's copies, so none is written.
    assert!(!without.join("build/cpp/std").exists());
}

#[test]
fn generated_cpp_snapshot() {
    let scratch = copy_fixture("hello", "dz_cpp_snapshot");
    let out = Command::new(dz())
        .args(["cpp", scratch.to_str().unwrap()])
        .output()
        .expect("failed to run dz");
    assert!(out.status.success());

    let dir = scratch.join("build/cpp");
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

/// The run fixtures whose C++ is compiled alone, warnings as errors: every one that
/// needs no include path beyond the generated files. `ownership` is the only program that
/// exercises the generated special members, and `failure` the only one that exercises `or`,
/// tagged unions and tuples. A warning in generated code is a compiler bug (invariant 7).
const WARNING_CLEAN: &[&str] = &[
    "features",
    "ownership",
    "failure",
    "hello",
    "keywords",
    "inherit",
    "switching",
    "loops",
    "button",
    "statics",
    "assoc",
    "dynamic",
    "flags",
    "location",
    "allocators",
    "containers",
    "argorder",
    "hashmap",
    "optionals",
    "unions",
    "iterable",
    "math",
    "variadic",
    "reflect",
    "handles",
    "time",
    "generic_methods",
    "random",
    "files",
    "utf8",
    "handle_upcast",
    "sorting",
    "breakout_sim",
    "text",
    "process",
    "formatting",
    "collections",
    "projection",
    "implicit_as",
    "ref_fields",
    "region_arena",
    "regions",
    "inline",
    "moves",
];

#[test]
fn the_generated_code_builds_clean_under_wall_wextra() {
    // spec §16.5 — nothing generated may need exceptions or RTTI, and it must not warn.
    // Every compiler present, not the first: the default build places values in regions,
    // and that C++ must be clean on clang++ and g++ alike (invariant 7, T-0152).
    let compilers: Vec<&str> = ["clang++", "g++"]
        .into_iter()
        .filter(
            |c| matches!(Command::new(c).arg("--version").output(), Ok(o) if o.status.success()),
        )
        .collect();
    if compilers.is_empty() {
        return;
    }

    // A scratch copy per fixture and one `dz cpp` each, so the whole list goes at once.
    parallel::for_each(WARNING_CLEAN, |fixture| check_warning_clean(&compilers, fixture));
}

/// Generate one fixture's C++, the default build, and compile every translation unit with
/// warnings as errors on each compiler.
fn check_warning_clean(compilers: &[&str], fixture: &str) {
    // Copied into a scratch project: the run test drives the same directories, and cargo
    // runs tests in parallel, so sharing a build directory would race.
    // The whole fixture, dependencies included, but not its `build`: that is what the run
    // test writes into.
    let dir = copy_fixture(fixture, &format!("dz_wall_wextra_{fixture}"));
    let out = Command::new(dz())
        .args(["cpp", dir.to_str().unwrap()])
        .env_remove("DZ_REGIONS")
        .output()
        .expect("failed to run dz");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    for cxx in compilers {
        assert_cpp_warning_clean(cxx, &dir.join("build/cpp"));
    }
}

/// Compile every translation unit in `dir` with warnings as errors (invariant 7).
fn assert_cpp_warning_clean(cxx: &str, dir: &Path) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let p = entry.path();
        if p.extension().is_some_and(|e| e == "cpp") {
            let out = Command::new(cxx)
                .args([
                    "-std=c++17",
                    "-fno-exceptions",
                    "-fno-rtti",
                    "-Wall",
                    "-Wextra",
                    "-fsyntax-only",
                ])
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
    // wrong. The deferral register in docs/tasks/ tracks these.
    let dir = std::env::temp_dir().join("dz_m4_unsupported");
    let _ = std::fs::create_dir_all(dir.join("src"));
    std::fs::write(
        dir.join("dazzle.toml"),
        "[module]\nname = \"u\"\nsources = [\"src/**/*.dz\"]\n\n[dependencies]\nstd = { open = true }\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/a.dz"),
        // A trait object of a trait whose method returns `Self`: a vtable cannot say what
        // that is, and without a DZ0801 it would reach C++ as a type nobody declared.
        "trait Shape {\n    fn area(&self) -> f32\n    fn cloned(&self) -> Self\n}\n\n\
         fn total(s: ^Shape) -> f32 {\n    return 0\n}\n\n\
         fn main() {\n    println!(\"{}\", 1)\n}\n",
    )
    .unwrap();

    let out =
        Command::new(dz()).args(["cpp", dir.to_str().unwrap()]).output().expect("failed to run dz");
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
        "[module]\nname = \"r\"\nsources = [\"src/**/*.dz\"]\n\n[dependencies]\nstd = { open = true }\n",
        "struct Box<T> {\n    pub v: T\n}\n\n\
         fn deeper<T>(x: T) -> i32 {\n    let b = Box<T>{ v: move(x) }\n    \
         return deeper(move(b))\n}\n\n\
         fn main() {\n    println!(\"{}\", deeper(1))\n}\n",
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
        "[module]\nname = \"n\"\nsources = [\"src/**/*.dz\"]\n\n[dependencies]\nstd = { open = true }\n",
    )
    .unwrap();
    std::fs::write(dir.join("src/a.dz"), "fn helper() -> i32 {\n    return 1\n}\n").unwrap();

    let out =
        Command::new(dz()).args(["cpp", dir.to_str().unwrap()]).output().expect("failed to run dz");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("DZ0802"), "expected DZ0802:\n{stderr}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_function_is_a_value() {
    // spec §5.4 — a function type is written `fn(i32) -> i32`, and C++ spells one
    // `R (*name)(A)`, with the name inside the parentheses. It reached the generated code
    // as `int32_t(*)(int32_t) g`, which does not compile; the shim's `dz::fn` alias is
    // what lets it be written where every other type goes.
    if !have_compiler() {
        return;
    }
    let dir = scratch(
        "dz_fn_value",
        "[module]\nname = \"f\"\nsources = [\"src/**/*.dz\"]\n\n[dependencies]\nstd = { open = true }\n",
        "fn twice(a: i32) -> i32 {\n    return a * 2\n}\n\n\
         fn apply(f: fn(i32) -> i32, v: i32) -> i32 {\n    return f(v)\n}\n\n\
         fn main() {\n    let g = twice\n    println!(\"{} {}\", apply(twice, 21), g(3))\n}\n",
    );
    let out = Command::new(dz()).args(["run", dir.to_str().unwrap()]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(normalise(&String::from_utf8_lossy(&out.stdout)), "42 6");
    let _ = std::fs::remove_dir_all(&dir);
}

// --- vendored libraries (spec §15.1) ------------------------------------------

/// Copy a fixture into a scratch directory, leaving out its `build/`, so a test that builds
/// it does not race the run test driving the same directory.
fn copy_fixture(name: &str, scratch_name: &str) -> PathBuf {
    fn copy_dir(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for e in std::fs::read_dir(from).unwrap().flatten() {
            let p = e.path();
            if p.file_name().is_some_and(|n| n == "build") {
                continue;
            }
            if p.is_dir() {
                copy_dir(&p, &to.join(e.file_name()));
            } else {
                std::fs::copy(&p, to.join(e.file_name())).unwrap();
            }
        }
    }
    let to = std::env::temp_dir().join(scratch_name);
    let _ = std::fs::remove_dir_all(&to);
    copy_dir(&run_dir(name), &to);
    // A dependency outside the fixture (`breakout_sim` plays `examples/breakout/game`) is
    // not copied with it, so its relative path is made absolute against the original.
    let manifest = to.join("dazzle.toml");
    let text = std::fs::read_to_string(&manifest).unwrap();
    let mut out = String::new();
    for line in text.lines() {
        match line.split_once("path = \"../") {
            Some((before, rest)) => {
                let (rel, after) = rest.split_once('"').unwrap();
                let abs = run_dir(name).join("..").join(rel).canonicalize().unwrap();
                // Forward slashes: a backslash in a TOML basic string is an escape.
                let abs = abs.to_string_lossy().trim_start_matches(r"\\?\").replace('\\', "/");
                out.push_str(&format!("{before}path = \"{abs}\"{after}\n"));
            }
            None => out.push_str(&format!("{line}\n")),
        }
    }
    std::fs::write(&manifest, out).unwrap();
    to
}

#[test]
fn a_vendored_library_is_compiled_once_and_again_when_a_header_changes() {
    // spec §15.1, §16.6 — an object is rebuilt only when something that went into it
    // changed, and a header it includes is one of those things.
    if !have_compiler() {
        return;
    }
    let dir = copy_fixture("cinterop", "dz_vendored_cache");
    let build = || {
        let out = Command::new(dz()).args(["build", dir.to_str().unwrap()]).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    };
    let objects = || {
        let mut found: Vec<(PathBuf, std::time::SystemTime)> =
            std::fs::read_dir(dir.join("build/obj/mini"))
                .unwrap()
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "o"))
                .map(|p| {
                    let t = std::fs::metadata(&p).unwrap().modified().unwrap();
                    (p, t)
                })
                .collect();
        found.sort();
        found
    };

    build();
    let first = objects();
    assert_eq!(first.len(), 1, "one source, one object: {first:?}");
    build();
    assert_eq!(objects(), first, "an unchanged library was recompiled");

    let header = dir.join("vendor/mini/include/mini.h");
    let mut text = std::fs::read_to_string(&header).unwrap();
    text.push_str("\n/* changed */\n");
    std::fs::write(&header, text).unwrap();
    build();
    assert_ne!(objects(), first, "a changed header did not recompile the library");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_cmake_library_is_built_once_and_again_when_its_source_changes() {
    // spec §15.1, D-0090 — CMake runs again only when something that went into the build
    // changed, judged by content, as a source recipe's objects are.
    if !have_compiler() || !have_cmake() {
        return;
    }
    let dir = copy_fixture("cmake", "dz_cmake_cache");
    let build = |args: &[&str]| {
        let out =
            Command::new(dz()).arg("build").args(args).arg(dir.to_str().unwrap()).output().unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
        assert!(out.status.success(), "{stderr}");
        stderr.contains("building tally with CMake")
    };
    assert!(build(&[]), "the first build did not run CMake");
    assert!(!build(&[]), "an unchanged library was built again");
    let source = dir.join("vendor/tally/tally.c");
    let text = std::fs::read_to_string(&source).unwrap();
    std::fs::write(&source, &text).unwrap();
    assert!(!build(&[]), "a file rewritten with the same content rebuilt the library");
    std::fs::write(&source, format!("{text}\n/* changed */\n")).unwrap();
    assert!(build(&[]), "a changed source did not rebuild the library");
    // Another configuration is a build tree of its own, and the first one is kept.
    assert!(build(&["--release"]), "a release build reused the debug library");
    assert!(!build(&[]), "switching to release and back rebuilt the debug library");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_cmake_library_is_built_again_when_its_project_moves() {
    // spec §15.1, D-0090 — CMake writes absolute paths into its cache and into the
    // install's pkg-config file, so a build made in another folder, by a checkout moved or
    // copied, is built again rather than used: its headers would be looked for where the
    // project was.
    if !have_compiler() || !have_cmake() {
        return;
    }
    fn copy_all(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for e in std::fs::read_dir(from).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                copy_all(&p, &to.join(e.file_name()));
            } else {
                std::fs::copy(&p, to.join(e.file_name())).unwrap();
            }
        }
    }
    let build = |dir: &Path| {
        let out = Command::new(dz()).arg("build").arg(dir).output().unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
        assert!(out.status.success(), "{stderr}");
        stderr.contains("building tally with CMake")
    };
    let from = copy_fixture("cmake", "dz_cmake_moved_from");
    assert!(build(&from), "the first build did not run CMake");
    // Moved: copied with its build folder, then the original removed.
    let to = std::env::temp_dir().join("dz_cmake_moved_to");
    let _ = std::fs::remove_dir_all(&to);
    copy_all(&from, &to);
    let _ = std::fs::remove_dir_all(&from);
    assert!(build(&to), "the moved project used the build made where it was");
    assert!(!build(&to), "an unchanged library was built again");
    // The same folder named another way is the same build.
    assert!(!build(&to.join("src").join("..")), "another spelling of the folder rebuilt it");
    let _ = std::fs::remove_dir_all(&to);
}

#[test]
fn a_cmake_build_that_fails_is_reported_at_its_recipe() {
    // DZ0708, with CMake's own words in the note, and nothing after it: every header the
    // build should have installed would only be reported missing too (D-0090).
    if !have_compiler() || !have_cmake() {
        return;
    }
    let dir = copy_fixture("cmake", "dz_cmake_broken");
    let lists = dir.join("vendor/tally/CMakeLists.txt");
    let text = std::fs::read_to_string(&lists).unwrap();
    std::fs::write(&lists, format!("{text}\nmessage(FATAL_ERROR \"tally is broken\")\n")).unwrap();
    let out = Command::new(dz())
        .args(["check", "--format=json", dir.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let text = json.to_string();
    assert!(text.contains("DZ0708"), "{text}");
    assert!(text.contains("tally is broken"), "{text}");
    assert!(!text.contains("DZ0709") && !text.contains("DZ0702"), "{text}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_library_that_fails_to_compile_is_blamed_on_the_library() {
    // DZ0803 says "this is a bug in the Dazzle compiler"; a vendored library's own error
    // must never be reported that way (spec §15.1).
    if !have_compiler() {
        return;
    }
    let dir = scratch(
        "dz_vendored_broken",
        "[module]\nname = \"b\"\nsources = [\"src/**/*.dz\"]\n\n[dependencies]\nstd = { open = true }\n\n\
         [cpp.libs.broken]\nsources = [\"broken.c\"]\n",
        "fn main() {\n    println!(\"{}\", 1)\n}\n",
    );
    std::fs::write(dir.join("broken.c"), "int broken(void) { return }\n").unwrap();
    let out = Command::new(dz()).args(["build", dir.to_str().unwrap()]).output().unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "expected the build to fail:\n{stderr}");
    assert!(stderr.contains("DZ0701"), "expected DZ0701:\n{stderr}");
    assert!(!stderr.contains("DZ0803"), "a library's error is not a Dazzle bug:\n{stderr}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_header_that_is_not_found_is_reported_at_the_import() {
    // spec §15.1 — clang's own words, attached to the line that asked for the header.
    let dir = scratch(
        "dz_import_missing",
        "[module]\nname = \"m\"\nsources = [\"src/**/*.dz\"]\n\n[dependencies]\nstd = { open = true }\nnope = { cpp = \"nope\" }\n\n[cpp.libs.nope]\nimport = [\"no_such_header.h\"]\n",
        "fn main() {\n    println!(\"{}\", 1)\n}\n",
    );
    let out = Command::new(dz()).args(["check", dir.to_str().unwrap()]).output().unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "expected the import to fail:\n{stderr}");
    // DZ0703 instead where this machine has no libclang: the import is refused either way.
    assert!(stderr.contains("DZ0702") || stderr.contains("DZ0703"), "{stderr}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_call_into_c_is_type_checked_like_any_other() {
    // spec §15.2, §18.37 — `int *` is a `^mut i32`, so a read-only pointer does not pass,
    // and there are no implicit numeric conversions across the boundary either.
    let dir = scratch(
        "dz_import_types",
        "[module]\nname = \"t\"\nsources = [\"src/**/*.dz\"]\n\n[dependencies]\nstd = { open = true }\napi = { cpp = \"api\" }\n\n[cpp.libs.api]\ninclude_dirs = [\".\"]\nimport = [\"api.h\"]\nkeeps = []\n",
        "fn main() {\n    let xs: [2]i32 = [1, 2]\n    api::fill(xs.ptr, 2)\n    \
         let n: i64 = 3\n    api::fill(null, n)\n}\n",
    );
    std::fs::write(dir.join("api.h"), "#pragma once\nextern \"C\" void fill(int *out, int n);\n")
        .unwrap();
    let out = Command::new(dz()).args(["check", dir.to_str().unwrap()]).output().unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    if stderr.contains("DZ0703") {
        return; // no libclang here
    }
    assert_eq!(stderr.matches("DZ0501").count(), 2, "expected two type mismatches:\n{stderr}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Spec §15.2, D-0094 point 6 (T-0135): a call that can hand C a pointer is `unsafe` until
/// the recipe vouches for the library with `keeps`, and a function `keeps` lists stays
/// `unsafe`. A function taking only values is safe either way. A name `keeps` lists must be
/// one the header declares.
#[test]
fn keeps_vouches_for_the_calls_that_take_a_pointer() {
    // `fill` takes a pointer and `scale` only a value: lines 3 and 4 of the program.
    let source = "fn main() {\n    let mut xs: [2]i32\n    api::fill(xs.ptr, 2)\n    \
                  let _ = api::scale(2)\n}\n";
    let header = "#pragma once\nextern \"C\" void fill(int *out, int n);\n\
                  extern \"C\" int scale(int v);\n";
    // Each diagnostic's code, and its line in whichever file it is in.
    let found = |keeps: &str| -> Option<Vec<(String, u32)>> {
        let dir = scratch(
            "dz_import_keeps",
            &format!(
                "[module]\nname = \"k\"\nsources = [\"src/**/*.dz\"]\n\n[dependencies]\n\
                 api = {{ cpp = \"api\" }}\n\n[cpp.libs.api]\ninclude_dirs = [\".\"]\n\
                 import = [\"api.h\"]\n{keeps}\n"
            ),
            source,
        );
        std::fs::write(dir.join("api.h"), header).unwrap();
        let out = Command::new(dz())
            .args(["check", "--format=json", dir.to_str().unwrap()])
            .output()
            .unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        let json = String::from_utf8_lossy(&out.stdout).into_owned();
        let field = |line: &str, key: &str| {
            let rest = &line[line.find(key)? + key.len()..];
            Some(rest.trim_start_matches([' ', '"']).split(['"', ',']).next()?.to_owned())
        };
        let found: Vec<(String, u32)> = json
            .lines()
            .filter_map(|l| Some((field(l, "\"code\":")?, field(l, "\"line\":")?.parse().ok()?)))
            .collect();
        // DZ0703: this machine has no libclang, so nothing here can be imported.
        (!found.iter().any(|(c, _)| c == "DZ0703")).then_some(found)
    };
    let Some(unvouched) = found("") else { return };
    assert_eq!(unvouched, [("DZ0528".to_owned(), 3)], "only the pointer call is refused");
    assert_eq!(found("keeps = []").unwrap(), [], "vouched for, both calls are safe");
    assert_eq!(
        found("keeps = [\"scale\"]").unwrap(),
        [("DZ0528".to_owned(), 4)],
        "a function `keeps` lists is unsafe, and the rest are vouched for"
    );
    // The recipe's `keeps` is on line 11 of the manifest.
    assert_eq!(found("keeps = [\"scael\"]").unwrap(), [("DZ0004".to_owned(), 11)]);
}

/// Spec §6.7, §7.7 (D-0094 point 7, T-0136): a clause names a static in another module as
/// an expression does, `world::SCORES`, and a caller that leaves its callee's kill out is
/// told to add it, spelt that way, which `dz fix` writes; the program then checks clean.
#[test]
fn a_clause_names_a_static_in_another_module() {
    let dir = scratch(
        "dz_static_clause",
        "[module]\nname = \"game\"\nsources = [\"src/**/*.dz\"]\n\n[dependencies]\n\
         std = { open = true }\nworld = { path = \"world\" }\n",
        "fn named(v: i32) kills world::SCORES.list@items {\n    world::add(v)\n}\n\n\
         fn unnamed(v: i32) {\n    world::add(v)\n}\n\n\
         fn main() {\n    named(1)\n    unnamed(2)\n}\n",
    );
    std::fs::create_dir_all(dir.join("world/src")).unwrap();
    std::fs::write(
        dir.join("world/dazzle.toml"),
        "[module]\nname = \"world\"\nsources = [\"src/**/*.dz\"]\n\n[dependencies]\n\
         std = { open = true }\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("world/src/lib.dz"),
        "pub struct Scores {\n    pub list: List<i32> = List<i32>()\n}\n\n\
         pub static SCORES: Scores = Scores{}\n\n\
         pub fn add(v: i32) kills SCORES.list@items {\n    SCORES.list.push(v)\n}\n",
    )
    .unwrap();
    let check = || {
        let out = Command::new(dz()).args(["check", dir.to_str().unwrap()]).output().unwrap();
        String::from_utf8_lossy(&out.stderr).into_owned()
    };
    let before = check();
    assert_eq!(before.matches("DZ0415").count(), 1, "{before}");
    assert!(before.contains("main.dz:6:"), "the kill is `unnamed`'s call:\n{before}");
    let out = Command::new(dz()).args(["fix", dir.to_str().unwrap()]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let main = std::fs::read_to_string(dir.join("src/main.dz")).unwrap();
    assert!(main.contains("fn unnamed(v: i32) kills world::SCORES.list@items {"), "{main}");
    let after = check();
    assert!(!after.contains("error"), "{after}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_imported_handle_cannot_be_held_by_value() {
    // spec §15.2, §18.40 — the header declares `Handle` and never defines it, so nothing
    // here knows its size. Behind a pointer it is exactly what the library hands out.
    let dir = scratch(
        "dz_import_handle",
        "[module]\nname = \"h\"\nsources = [\"src/**/*.dz\"]\n\n[dependencies]\nstd = { open = true }\napi = { cpp = \"api\" }\n\n[cpp.libs.api]\ninclude_dirs = [\".\"]\nimport = [\"api.h\"]\n",
        "fn keep(h: ^api::Handle) -> ^api::Handle {\n    return h\n}\n\n\
         fn main() {\n    let h = api::api_open()\n    let _ = keep(h)\n    \
         let bad: api::Handle\n}\n",
    );
    std::fs::write(
        dir.join("api.h"),
        "#pragma once\ntypedef struct Handle Handle;\nextern \"C\" Handle *api_open(void);\n",
    )
    .unwrap();
    let out = Command::new(dz()).args(["check", dir.to_str().unwrap()]).output().unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    if stderr.contains("DZ0703") {
        return; // no libclang here
    }
    assert_eq!(
        stderr.matches("DZ0704").count(),
        1,
        "expected one handle-by-value error:\n{stderr}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_c_union_literal_sets_one_member() {
    // spec §15.2, §18.41 — the members are the same memory, so setting two writes the same
    // bytes twice and only the last survives.
    let dir = scratch(
        "dz_import_union",
        "[module]\nname = \"u\"\nsources = [\"src/**/*.dz\"]\n\n[dependencies]\nstd = { open = true }\napi = { cpp = \"api\" }\n\n[cpp.libs.api]\ninclude_dirs = [\".\"]\nimport = [\"api.h\"]\n",
        "fn main() {\n    let one = api::Value{ as_int: 1 }\n    \
         let two = api::Value{ as_int: 1, as_float: 2.0 }\n    \
         println!(\"{} {}\", one.as_int, two.as_int)\n}\n",
    );
    std::fs::write(
        dir.join("api.h"),
        "#pragma once\ntypedef union Value { int as_int; float as_float; } Value;\n",
    )
    .unwrap();
    let out = Command::new(dz()).args(["check", dir.to_str().unwrap()]).output().unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    if stderr.contains("DZ0703") {
        return; // no libclang here
    }
    assert_eq!(stderr.matches("DZ0705").count(), 1, "expected one union-literal error:\n{stderr}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_c_bit_field_has_no_address() {
    // spec §15.2 — a bit-field is a few bits inside another member, and C++ has no pointer
    // or reference to one. The ordinary field beside it is unaffected.
    let dir = scratch(
        "dz_import_bitfield",
        "[module]\nname = \"b\"\nsources = [\"src/**/*.dz\"]\n\n[dependencies]\nstd = { open = true }\napi = { cpp = \"api\" }\n\n[cpp.libs.api]\ninclude_dirs = [\".\"]\nimport = [\"api.h\"]\n",
        "fn main() {\n    let mut bits: api::Bits\n    let on = &mut bits.on\n    \
         let count = &mut bits.count\n    println!(\"{}\", bits.count)\n}\n",
    );
    std::fs::write(
        dir.join("api.h"),
        "#pragma once\ntypedef struct Bits { unsigned int on : 1; int count; } Bits;\n",
    )
    .unwrap();
    let out = Command::new(dz()).args(["check", dir.to_str().unwrap()]).output().unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    if stderr.contains("DZ0703") {
        return; // no libclang here
    }
    assert_eq!(stderr.matches("DZ0706").count(), 1, "expected one bit-field error:\n{stderr}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_c_array_member_is_indexed_not_copied() {
    // spec §15.2 — a raw array cannot be copied or assigned in C++, so taking one as a
    // value is refused here rather than emitted as code that will not compile.
    let dir = scratch(
        "dz_import_carray",
        "[module]\nname = \"a\"\nsources = [\"src/**/*.dz\"]\n\n[dependencies]\nstd = { open = true }\napi = { cpp = \"api\" }\n\n[cpp.libs.api]\ninclude_dirs = [\".\"]\nimport = [\"api.h\"]\n",
        "fn main() {\n    let box: api::Box\n    let first = box.items[0]\n    \
         let n = box.items.len\n    let copied = box.items\n    \
         println!(\"{} {} {}\", first, n, copied[0])\n}\n",
    );
    std::fs::write(dir.join("api.h"), "#pragma once\ntypedef struct Box { int items[4]; } Box;\n")
        .unwrap();
    let out = Command::new(dz()).args(["check", dir.to_str().unwrap()]).output().unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    if stderr.contains("DZ0703") {
        return; // no libclang here
    }
    // Indexing and `.len` are fine; only the copy is refused.
    assert_eq!(stderr.matches("DZ0707").count(), 1, "expected one array-copy error:\n{stderr}");
    let _ = std::fs::remove_dir_all(&dir);
}

// --- code quality ---------------------------------------------------------

/// Write a throwaway project and return its directory.
/// `type_info` decides what the table holds (spec §13.2): with `"none"` a reflection call
/// is an error; with `"all"` every declared type is described, a generic instance and an
/// imported header's type among what that reaches, and the table still compiles
/// warning-clean on both compilers (invariant 7).
#[test]
fn type_info_none_refuses_reflection_and_all_describes_everything() {
    let source = "struct Probe {\n    pub a: i32 = 0\n    pub bytes: List<u8>\n}\n\n\
                  struct Holder<T> {\n    pub item: T\n}\n\n\
                  union Shape {\n    Circle { radius: f32 }\n    Dot {}\n}\n\n\
                  fn main() {\n    println!(\"{}\", typeinfo_of<Probe>().name)\n    \
                  let h = Holder<List<u8>>{ item: List<u8>() }\n    println!(\"{}\", h.item.len())\n    \
                  let s = Shape::Dot{}\n    let _ = s\n}\n";
    let none = scratch(
        "dz_type_info_none",
        "[module]\nname = \"n\"\nsources = [\"src/**/*.dz\"]\n\n[dependencies]\nstd = { open = true }\n\n[build]\ntype_info = \"none\"\n",
        source,
    );
    let out = Command::new(dz()).args(["check", none.to_str().unwrap()]).output().unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success() && stderr.contains("DZ0555"), "{stderr}");
    let _ = std::fs::remove_dir_all(&none);

    let Some(cxx) = ["clang++", "g++"]
        .iter()
        .find(|c| matches!(Command::new(c).arg("--version").output(), Ok(o) if o.status.success()))
    else {
        return;
    };
    let all = scratch(
        "dz_type_info_all",
        "[module]\nname = \"a\"\nsources = [\"src/**/*.dz\"]\n\n[dependencies]\nstd = { open = true }\n\n[build]\ntype_info = \"all\"\n",
        source,
    );
    let out = Command::new(dz()).args(["cpp", all.to_str().unwrap()]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let table = all.join("build/cpp/dz_typeinfo.cpp");
    let text = std::fs::read_to_string(&table).expect("the table is written");
    // Every declared type is in it, `Shape` included though nothing names it.
    for name in ["\"Probe\"", "\"Shape\"", "\"List<u8>\"", "\"TypeInfo\""] {
        assert!(text.contains(name), "`{name}` missing from the table");
    }
    let out = Command::new(cxx)
        .args(["-std=c++17", "-fno-exceptions", "-fno-rtti", "-Wall", "-Wextra", "-fsyntax-only"])
        .arg(&table)
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success() && stderr.trim().is_empty(), "{stderr}");
    let _ = std::fs::remove_dir_all(&all);
}

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
    println!(\"{} {}\", a.x, a.y)
}
";

#[test]
fn cpp20_uses_designated_initialisers_and_cpp17_does_not() {
    for (std_name, expect_designated) in [("c++17", false), ("c++20", true)] {
        let manifest = format!(
            "[module]\nname = \"lit\"\nsources = [\"src/**/*.dz\"]\n\n[dependencies]\nstd = {{ open = true }}\n\n\
             [build]\noutput = \"executable\"\ncpp_standard = \"{std_name}\"\n"
        );
        let dir =
            scratch(&format!("dz_lit_{}", std_name.replace("+", "p")), &manifest, LITERAL_SRC);
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
        "[module]\nname = \"perf\"\nsources = [\"src/**/*.dz\"]\n\n[dependencies]\nstd = { open = true }\n",
        "fn sum(xs: &[]i32) -> i32 {\n    let mut total = 0\n    for i in 0..xs.len {\n        \
         total += xs[i]\n    }\n    return total\n}\n\nfn main() {\n    println!(\"{}\", 0)\n}\n",
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
                (l.contains("obj@")
                    || l.contains("3obj")
                    || l.contains("index@")
                    || l.contains("5index"))
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

/// std's `kills` clauses over `unsafe` code are trusted by the checker, and checked when a
/// build sets `check_trusted` (spec §6.7, T-0116): every guarded function walks the blocks
/// its containers' elements own and traps if one it promised to keep was freed or handed
/// out again. The programs that exercise the containers run with it on and must print what
/// they print without it, which is the corpus holding std to its clauses.
#[test]
fn std_clauses_hold_under_check_trusted() {
    if !have_compiler() {
        eprintln!("skipping: no C++ compiler available");
        return;
    }
    for fixture in [
        "collections",
        "hashmap",
        "slot_map",
        "iterable",
        "ref_fields",
        "sorting",
        "text",
        "projection",
        "iterator",
        "containers",
        "optionals",
    ] {
        let dir = copy_fixture(fixture, &format!("dz_check_trusted_{fixture}"));
        let manifest = dir.join("dazzle.toml");
        let text = std::fs::read_to_string(&manifest).unwrap();
        let text = if text.contains("[build]") {
            text.replacen("[build]", "[build]\ncheck_trusted = true", 1)
        } else {
            format!("{text}\n[build]\ncheck_trusted = true\n")
        };
        std::fs::write(&manifest, text).unwrap();
        let out = Command::new(dz()).args(["run", dir.to_str().unwrap()]).output().expect("run dz");
        let stdout = normalise(&String::from_utf8_lossy(&out.stdout));
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "{fixture} under check_trusted:\n{stderr}");
        let expected =
            normalise(&std::fs::read_to_string(run_dir(fixture).join("expected.txt")).unwrap());
        assert_eq!(stdout, expected, "{fixture} under check_trusted");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
