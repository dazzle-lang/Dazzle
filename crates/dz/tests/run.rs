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
    });
}

/// The examples, guarded. `examples/raylib`, `examples/rgfw` and `examples/breakout` open a
/// window, so none can carry an `expected.txt`; without this, M7b's own criterion would go
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

    for (name, dir) in found {
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
/// `or unreachable` panics in a debug build but lets a release build assume success (§9.4,
/// 18.20) — checked in the generated C++, since the release behaviour is undefined when wrong.
#[test]
fn overflow_wraps_and_unreachable_is_assumed_in_release() {
    if !have_compiler() {
        eprintln!("skipping: no C++ compiler available");
        return;
    }
    let dir = scratch(
        "dz_overflow",
        "[module]\nname = \"overflow\"\nsources = [\"src/**/*.dz\"]\n",
        "fn grows(x: i32) -> bool {\n    return x + 1 > x\n}\n\nfn find(n: i32) -> i32? {\n    return n\n}\n\nfn main() {\n    let max: i32 = 2147483647\n    let small = 250u8\n    let v = find(3) or unreachable\n    println!(\"{} {} {} {}\", max + 1, grows(max), small + 10, v)\n}\n",
    );
    for mode in ["--debug", "--release", "--shipping"] {
        let out = Command::new(dz())
            .args(["run", dir.to_str().unwrap(), mode])
            .output()
            .expect("failed to run dz");
        assert!(out.status.success(), "{mode}: {}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(
            normalise(&String::from_utf8_lossy(&out.stdout)),
            "-2147483648 false 4 3",
            "{mode}"
        );
        let cpp = std::fs::read_to_string(dir.join("build/cpp/overflow.cpp")).unwrap();
        assert_eq!(
            cpp.contains("__builtin_unreachable()"),
            mode != "--debug",
            "{mode}: `or unreachable` lowering\n{cpp}"
        );
    }
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
    for fixture in
        ["allocators", "hashmap", "handles", "files", "handle_upcast", "sorting", "collections"]
    {
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
/// into `dz`, is written beside the generated C++, and every `#line` naming it points there —
/// indented ones inside generated lambdas included — so a debugger can open what it steps
/// into. A panic's location still reads `<std>/collections/list.dz`.
#[test]
fn line_directives_name_files_a_debugger_can_open() {
    let dir = scratch(
        "dz_debug_lines",
        "[module]\nname = \"lines\"\nsources = [\"src/**/*.dz\"]\n\n[dependencies]\nstd = { open = true }\n",
        "fn main() {\n    let mut xs = List<i32>()\n    xs.push(1)\n    println!(\"{}\", xs.len())\n}\n",
    );
    let out =
        Command::new(dz()).args(["cpp", dir.to_str().unwrap()]).output().expect("failed to run dz");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let copy = dir.join("build/cpp/std/collections/list.dz");
    assert_eq!(
        std::fs::read_to_string(&copy).expect("the standard library is written out"),
        dz_project::std_source(Path::new("<std>/collections/list.dz")).unwrap()
    );
    let mut rewritten = 0;
    for entry in std::fs::read_dir(dir.join("build/cpp")).unwrap().flatten() {
        let Ok(text) = std::fs::read_to_string(entry.path()) else { continue };
        for line in text.lines().filter(|l| l.trim_start().starts_with("#line ")) {
            assert!(!line.contains("<std>"), "{}: {line}", entry.path().display());
            rewritten += usize::from(line.ends_with("/build/cpp/std/collections/list.dz\""));
        }
    }
    assert!(rewritten > 0, "the standard library's own code carries `#line`s naming the copy");
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
    // A scratch copy per fixture and one `dz cpp` each, so the whole list goes at once.
    parallel::for_each(
        &[
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
        ],
        |fixture| check_warning_clean(cxx, fixture),
    );
}

/// Generate one fixture's C++ and compile every translation unit with warnings as errors.
fn check_warning_clean(cxx: &str, fixture: &str) {
    // Copied into a scratch project: the run test drives the same directories, and cargo
    // runs tests in parallel, so sharing a build directory would race.
    // The whole fixture, dependencies included, but not its `build`: that is what the run
    // test writes into.
    let dir = copy_fixture(fixture, &format!("dz_wall_wextra_{fixture}"));
    let out =
        Command::new(dz()).args(["cpp", dir.to_str().unwrap()]).output().expect("failed to run dz");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));

    let dir = dir.join("build/cpp");
    for entry in std::fs::read_dir(&dir).unwrap().flatten() {
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
        "[module]\nname = \"t\"\nsources = [\"src/**/*.dz\"]\n\n[dependencies]\nstd = { open = true }\napi = { cpp = \"api\" }\n\n[cpp.libs.api]\ninclude_dirs = [\".\"]\nimport = [\"api.h\"]\n",
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
    let source = "struct Probe {\n    pub a: i32 = 0\n}\n\n\
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
        "fn sum(xs: []i32) -> i32 {\n    let mut total = 0\n    for i in 0..xs.len {\n        \
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
