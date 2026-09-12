//! The `dz` command line driver.
//!
//! `lex`, `parse` and `fmt` exist at M1. The full subcommand set is in spec §17.3.

use std::path::Path;
use std::process::ExitCode;

mod build;
mod native;

use dz_project::Optimize;
use dz_span::{FileId, SourceMap};

const USAGE: &str = "\
dz — the Dazzle compiler

usage:
    dz run   [path]             build a project and execute it
    dz build [path]             generate C++ and compile it
    dz cpp   [path]             generate C++ only, no compilation
    dz test  [path]             build the project's `#[test]` functions and run them
    dz bench [path]             build the project's `#[bench]` functions and time them
    dz check [path]             load a project and check it without generating code
    dz hir   [path]             check and print the typed tree
    dz lex   <file.dz>          tokenize and print the token stream
    dz parse <file.dz>          parse and print the syntax tree
    dz fmt   <file.dz>...       format in place
    dz fmt --check <file.dz>... report which files are not formatted (exit 1 if any)
    dz fmt --stdout <file.dz>   print the formatted source without writing
    dz lsp                      run the language server on stdin/stdout
    dz --version
    dz --help

`path` is a dazzle.toml or a directory containing one; it defaults to the current
directory. `expand` is not implemented yet.

`test` and `bench` take a filter: `dz test . name` runs only the ones whose name
contains `name`. Both are functions written beside the code they cover — a
`#[test]` passes by returning and fails by panicking, a `#[bench]` is timed over
as many iterations as it takes to measure — and a normal build leaves both out.
`bench` builds `--release` unless told otherwise, since timing `-O0` measures
nothing anyone ships.

`run`, `build` and `cpp` take a build mode, which overrides the manifest's
`[build] optimize` for that one command:

    --debug                     -O0 -g  (the default)
    --release                   -O2
    --shipping                  -O3 -DNDEBUG";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("check") => cmd_check(args.get(1).map(String::as_str).unwrap_or(".")),
        Some("hir") => cmd_hir(args.get(1).map(String::as_str).unwrap_or(".")),
        Some("cpp") => with_mode(&args, Stage::Cpp),
        Some("build") => with_mode(&args, Stage::Build),
        Some("run") => with_mode(&args, Stage::Run),
        Some("test") => cmd_harness(&args, dz_cpp::Mode::Tests),
        Some("bench") => cmd_harness(&args, dz_cpp::Mode::Benches),
        Some("lex") => with_file(&args, cmd_lex),
        Some("parse") => with_file(&args, cmd_parse),
        Some("fmt") => cmd_fmt(&args[1..]),
        Some("lsp") => cmd_lsp(),
        Some("--version" | "-V") => {
            println!("dz {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some("--help" | "-h") | None => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("error: unknown subcommand `{other}`\n\n{USAGE}");
            ExitCode::FAILURE
        }
    }
}

fn with_file(args: &[String], f: impl Fn(&str) -> ExitCode) -> ExitCode {
    match args.get(1) {
        Some(path) => f(path),
        None => {
            eprintln!("error: `dz {}` needs a file\n\n{USAGE}", args[0]);
            ExitCode::FAILURE
        }
    }
}

/// Read a file into a fresh [`SourceMap`], reporting encoding problems.
fn load(path: &str) -> Option<(SourceMap, FileId)> {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("error: cannot read {path}: {e}");
            return None;
        }
    };
    let mut map = SourceMap::new();
    match map.add_bytes(path, &bytes) {
        Ok(id) => Some((map, id)),
        Err(e) => {
            eprintln!("error[{}]: {e}", dz_diag::codes::BAD_ENCODING);
            None
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Cpp,
    Build,
    Run,
}

/// The directory a project's `build/` should live in.
fn project_root(entry: &str) -> std::path::PathBuf {
    let p = Path::new(entry);
    if p.is_dir() {
        p.to_path_buf()
    } else {
        p.parent().unwrap_or(Path::new(".")).to_path_buf()
    }
}

/// Print the typed tree the checker built (spec §17.1). A development aid: it is how you
/// see what the compiler actually concluded, rather than inferring it from the C++.
fn cmd_hir(entry: &str) -> ExitCode {
    let analysis = build::analyse(Path::new(entry));
    if !analysis.diags.is_empty() {
        dz_diag::emit_to_stderr(&analysis.map, &analysis.diags);
    }
    if analysis.diags.has_errors() {
        return failure(&analysis.diags);
    }
    match (&analysis.hir, &analysis.resolution) {
        (Some(hir), Some(res)) => {
            print!("{}", dz_hir::dump::dump(hir, &dz_check::Names { res }));
            ExitCode::SUCCESS
        }
        _ => ExitCode::FAILURE,
    }
}

fn cmd_check(entry: &str) -> ExitCode {
    let analysis = build::analyse(Path::new(entry));
    if !analysis.diags.is_empty() {
        dz_diag::emit_to_stderr(&analysis.map, &analysis.diags);
    }
    if analysis.diags.has_errors() {
        return failure(&analysis.diags);
    }
    let modules = analysis.ws.as_ref().map(|w| w.len()).unwrap_or(0);
    let files: usize = analysis.files.values().map(Vec::len).sum();
    println!(
        "checked {modules} module{}, {files} file{}",
        if modules == 1 { "" } else { "s" },
        if files == 1 { "" } else { "s" }
    );
    ExitCode::SUCCESS
}

/// `dz test` and `dz bench` — build a harness binary and drive it (spec §19 M14, M15).
///
/// Each test runs in its own process. A failing test panics, a panic aborts (§9.8) and
/// there are no exceptions to catch (§16.5), so calling them all in one process would stop
/// at the first failure and report nothing about the rest. Spawning once per test costs a
/// process and buys a complete report, which is what a test runner is for.
fn cmd_harness(args: &[String], kind: dz_cpp::Mode) -> ExitCode {
    let benching = kind == dz_cpp::Mode::Benches;
    let what = if benching { "benchmark" } else { "test" };
    let mut path = ".";
    let mut filter: Option<&str> = None;
    // Timing an unoptimised build measures the debug build and nothing else, so
    // `dz bench` optimises by default; `dz bench --debug` is still available for
    // anyone who wants to see what the debug build costs.
    let mut mode: Option<Optimize> = if benching { Some(Optimize::Release) } else { None };
    let mut positional = 0;
    for a in &args[1..] {
        match a.as_str() {
            "--debug" => mode = Some(Optimize::Debug),
            "--release" => mode = Some(Optimize::Release),
            "--shipping" => mode = Some(Optimize::Shipping),
            other if other.starts_with("--") => {
                eprintln!("error: unknown option `{other}`\n\n{USAGE}");
                return ExitCode::FAILURE;
            }
            other => {
                if positional == 0 {
                    path = other;
                } else {
                    filter = Some(other);
                }
                positional += 1;
            }
        }
    }

    let mut analysis = build::analyse(Path::new(path));
    if let (Some(m), Some(ws)) = (mode, analysis.ws.as_mut()) {
        for module in ws.modules_mut() {
            module.build.optimize = m;
        }
    }
    let analysis = analysis;
    if !analysis.diags.is_empty() {
        dz_diag::emit_to_stderr(&analysis.map, &analysis.diags);
    }
    if analysis.diags.has_errors() {
        return failure(&analysis.diags);
    }

    let count = analysis
        .table
        .as_ref()
        .map(|t| if benching { t.benches.len() } else { t.tests.len() })
        .unwrap_or(0);
    if count == 0 {
        let attr = if benching { "bench" } else { "test" };
        println!(
            "no {what}s found; write `#[{attr}] fn name() {{ … }}` beside the code it covers"
        );
        return ExitCode::SUCCESS;
    }

    // Each harness builds into its own directory, so building tests never overwrites the
    // program's objects, benchmarks never overwrite the tests', and `dz run` afterwards
    // does not relink somebody else's entry point.
    let dir = if benching { "bench" } else { "test" };
    let root = project_root(path).join("build").join(dir);
    let out = match build::emit_cpp(&analysis, &root, kind) {
        Ok(o) => o,
        Err(d) => {
            dz_diag::emit_to_stderr(&analysis.map, &d);
            return failure(&d);
        }
    };
    let exe = match build::compile(&analysis, &out, &root) {
        Ok(e) => e,
        Err(d) => {
            dz_diag::emit_to_stderr(&analysis.map, &d);
            return failure(&d);
        }
    };

    let listed = match std::process::Command::new(&exe).arg("--list").output() {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
        Ok(o) => {
            eprintln!("error: the {what} binary could not list its {what}s: {}", o.status);
            return ExitCode::FAILURE;
        }
        Err(e) => {
            eprintln!("error: cannot run {}: {e}", exe.display());
            return ExitCode::FAILURE;
        }
    };

    let names: Vec<&str> = listed
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .filter(|l| filter.map_or(true, |f| l.contains(f)))
        .collect();
    if names.is_empty() {
        println!("no {what} matches `{}`", filter.unwrap_or(""));
        return ExitCode::SUCCESS;
    }

    println!(
        "running {} {what}{}",
        names.len(),
        if names.len() == 1 { "" } else { "s" }
    );
    let mut failed = Vec::new();
    for name in &names {
        match std::process::Command::new(&exe).arg(name).output() {
            Ok(o) if o.status.success() => {
                if benching {
                    // The harness prints "<ns per iteration> <iterations>"; the driver
                    // decides how to show it, so the number stays raw on the wire.
                    let line = String::from_utf8_lossy(&o.stdout);
                    let mut parts = line.split_whitespace();
                    match (parts.next(), parts.next()) {
                        (Some(per), Some(iters)) => {
                            println!("  {name:<40} {per:>12} ns/iter  ({iters} iterations)")
                        }
                        _ => println!("  {name:<40} (no measurement)"),
                    }
                } else {
                    println!("  {:<6}{name}", "ok");
                }
            }
            Ok(o) => {
                println!("  {:<6}{name}", if benching { "FAILED" } else { "FAIL" });
                // The panic message is the code's own report of what went wrong, so it
                // goes through rather than being summarised away.
                let stderr = String::from_utf8_lossy(&o.stderr);
                for line in stderr.lines() {
                    println!("          {line}");
                }
                failed.push(*name);
            }
            Err(e) => {
                println!("  ERROR {name}: {e}");
                failed.push(*name);
            }
        }
    }

    let passed = names.len() - failed.len();
    if failed.is_empty() {
        if benching {
            println!("\n{passed} benchmark{} measured", if passed == 1 { "" } else { "s" });
        } else {
            println!("\n{passed} passed");
        }
        ExitCode::SUCCESS
    } else {
        println!("\n{passed} passed, {} failed:", failed.len());
        for f in &failed {
            println!("  {f}");
        }
        ExitCode::FAILURE
    }
}

/// Split `run`/`build`/`cpp`'s arguments into a path and a build mode.
///
/// The mode overrides the manifest's `[build] optimize` for one command, so a project can
/// keep `debug` as what it is worked on in while `dz run --release` measures it. Written
/// as a flag rather than a subcommand because it is the same command either way.
fn with_mode(args: &[String], stage: Stage) -> ExitCode {
    let mut path = ".";
    let mut mode: Option<Optimize> = None;
    for a in &args[1..] {
        match a.as_str() {
            "--debug" => mode = Some(Optimize::Debug),
            "--release" => mode = Some(Optimize::Release),
            "--shipping" => mode = Some(Optimize::Shipping),
            other if other.starts_with("--") => {
                eprintln!("error: unknown option `{other}`\n\n{USAGE}");
                return ExitCode::FAILURE;
            }
            other => path = other,
        }
    }
    cmd_pipeline(path, stage, mode)
}

/// `dz cpp`, `dz build` and `dz run` share everything up to where they stop.
fn cmd_pipeline(entry: &str, stage: Stage, mode: Option<Optimize>) -> ExitCode {
    let mut analysis = build::analyse(Path::new(entry));
    if let (Some(m), Some(ws)) = (mode, analysis.ws.as_mut()) {
        // Every module, not just the root: one command builds one program, and a
        // dependency compiled at a different optimisation level is the classic way to get
        // a binary that behaves differently from either setting.
        for module in ws.modules_mut() {
            module.build.optimize = m;
        }
    }
    let analysis = analysis;
    if !analysis.diags.is_empty() {
        dz_diag::emit_to_stderr(&analysis.map, &analysis.diags);
    }
    if analysis.diags.has_errors() {
        return failure(&analysis.diags);
    }

    // The output directory, not the project root: `emit_cpp` and `compile` write straight
    // into it, so a harness can put its own elsewhere without nesting a second `build/`
    // inside the first.
    let root = project_root(entry).join("build");
    let out = match build::emit_cpp(&analysis, &root, dz_cpp::Mode::Program) {
        Ok(o) => o,
        Err(d) => {
            dz_diag::emit_to_stderr(&analysis.map, &d);
            return failure(&d);
        }
    };

    if stage == Stage::Cpp {
        println!(
            "generated {} file{} in {}",
            out.written.len(),
            if out.written.len() == 1 { "" } else { "s" },
            out.cpp_dir.display()
        );
        return ExitCode::SUCCESS;
    }

    let exe = match build::compile(&analysis, &out, &root) {
        Ok(e) => e,
        Err(d) => {
            dz_diag::emit_to_stderr(&analysis.map, &d);
            return failure(&d);
        }
    };

    if stage == Stage::Build {
        println!("built {}", exe.display());
        return ExitCode::SUCCESS;
    }

    match build::run(&exe) {
        Ok(status) => {
            if status.success() {
                ExitCode::SUCCESS
            } else {
                // Pass the program's own exit code through, so `dz run` is transparent.
                //
                // A failing status must never come back as 0. Windows reports an abort
                // as 0xC0000409, which is negative as an `i32` and does not fit in a
                // `u8`; anything that cannot be represented becomes 1 rather than being
                // clamped, which would have turned a crash into success.
                let code = status
                    .code()
                    .and_then(|c| u8::try_from(c).ok())
                    .filter(|c| *c != 0)
                    .unwrap_or(1);
                ExitCode::from(code)
            }
        }
        Err(e) => {
            eprintln!("error: could not run {}: {e}", exe.display());
            ExitCode::FAILURE
        }
    }
}

/// The language server (spec §17.4). It speaks the protocol on stdout, so nothing else
/// may print there.
fn cmd_lsp() -> ExitCode {
    match dz_lsp::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: language server: {e}");
            ExitCode::FAILURE
        }
    }
}

fn failure(diags: &dz_diag::Diagnostics) -> ExitCode {
    let n = diags.error_count();
    if n > 0 {
        eprintln!("
{n} error{} emitted", if n == 1 { "" } else { "s" });
    }
    ExitCode::FAILURE
}

fn cmd_lex(path: &str) -> ExitCode {
    let Some((map, id)) = load(path) else { return ExitCode::FAILURE };
    let file = map.get(id);
    let result = dz_lex::lex(id, file.text());

    print!("{}", dz_lex::dump_tokens(file, &result.tokens));

    let mut diags = result.diagnostics;
    diags.sort_by_position();
    report(&map, &diags)
}

fn cmd_parse(path: &str) -> ExitCode {
    let Some((map, id)) = load(path) else { return ExitCode::FAILURE };
    let file = map.get(id);
    let lexed = dz_lex::lex(id, file.text());
    let parsed = dz_parse::parse(id, file.text(), &lexed.tokens);

    print!("{}", dz_ast::dump::dump(file.text(), &parsed.ast));

    let mut diags = lexed.diagnostics;
    diags.extend(parsed.diagnostics);
    diags.sort_by_position();
    report(&map, &diags)
}

fn cmd_fmt(args: &[String]) -> ExitCode {
    let mut check = false;
    let mut to_stdout = false;
    let mut paths: Vec<&str> = Vec::new();
    for a in args {
        match a.as_str() {
            "--check" => check = true,
            "--stdout" => to_stdout = true,
            other if other.starts_with("--") => {
                eprintln!("error: unknown flag `{other}`\n\n{USAGE}");
                return ExitCode::FAILURE;
            }
            other => paths.push(other),
        }
    }
    if paths.is_empty() {
        eprintln!("error: `dz fmt` needs at least one file\n\n{USAGE}");
        return ExitCode::FAILURE;
    }

    let mut failed = false;
    let mut unformatted = Vec::new();

    for path in paths {
        let Some((map, id)) = load(path) else {
            failed = true;
            continue;
        };
        let file = map.get(id);
        let lexed = dz_lex::lex(id, file.text());
        let parsed = dz_parse::parse(id, file.text(), &lexed.tokens);

        let mut diags = lexed.diagnostics;
        diags.extend(parsed.diagnostics);
        // Formatting source with syntax errors would silently discard the broken parts,
        // so refuse rather than mangle the file.
        if diags.has_errors() {
            diags.sort_by_position();
            eprintln!("error: {path} has syntax errors and was not formatted");
            dz_diag::emit_to_stderr(&map, &diags);
            failed = true;
            continue;
        }

        let formatted = dz_fmt::format(file, &parsed.ast, &lexed.comments);

        if to_stdout {
            print!("{formatted}");
        } else if check {
            if formatted != file.text() {
                unformatted.push(path.to_owned());
            }
        } else if formatted != file.text() {
            if let Err(e) = std::fs::write(path, &formatted) {
                eprintln!("error: cannot write {path}: {e}");
                failed = true;
            } else {
                println!("formatted {path}");
            }
        }
    }

    if !unformatted.is_empty() {
        for p in &unformatted {
            println!("not formatted: {p}");
        }
        return ExitCode::FAILURE;
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn report(map: &SourceMap, diags: &dz_diag::Diagnostics) -> ExitCode {
    if diags.is_empty() {
        return ExitCode::SUCCESS;
    }
    println!();
    dz_diag::emit_to_stderr(map, diags);
    if diags.has_errors() {
        eprintln!(
            "\n{} error{} emitted",
            diags.error_count(),
            if diags.error_count() == 1 { "" } else { "s" }
        );
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
