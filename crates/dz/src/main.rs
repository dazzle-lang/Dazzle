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
    dz check --format=json      the same, every diagnostic as JSON on stdout
    dz fix   [path]             apply the fixes the compiler is certain of
    dz fix --dry-run [path]     list them, changing nothing
    dz hir   [path]             check and print the typed tree
    dz lex   <file.dz>          tokenize and print the token stream
    dz parse <file.dz>          parse and print the syntax tree
    dz expand <file.dz>         print the file with every macro invocation expanded
    dz fmt   <file.dz>...       format in place
    dz fmt --check <file.dz>... report which files are not formatted (exit 1 if any)
    dz fmt --stdout <file.dz>   print the formatted source without writing
    dz explain <code>           what a diagnostic means, with a wrong and a right program
    dz explain                  every diagnostic code, one line each
    dz lsp                      run the language server on stdin/stdout
    dz --version
    dz --help

`path` is a dazzle.toml or a directory containing one; it defaults to the current
directory. `expand` finds the project the file belongs to by looking for the
nearest dazzle.toml above it, since a macro may come from a dependency.

`test` and `bench` take a filter: `dz test . name` runs only the ones whose name
contains `name`. Both are functions written beside the code they cover — a
`#[test]` passes by returning and fails by panicking, a `#[bench]` is timed over
as many iterations as it takes to measure — and a normal build leaves both out.
`bench` builds `--release` unless told otherwise, since timing `-O0` measures
nothing anyone ships.

`run`, `build`, `cpp`, `test` and `bench` take a build mode, which overrides the
manifest's `[build] optimize` for that one command:

    --debug                     -O0 -g  (the default)
    --release                   -O2
    --shipping                  -O3 -DNDEBUG

and build flags, which `#if` reads (spec §12.10):

    --flag NAME                 turn on the package's flag NAME
    --flag engine::NAME         turn on a dependency's flag
    --track-memory              record every allocation; report leaks and bad frees

They also take `--cxx <compiler>`, which is the last word on which C++ compiler
builds the generated code. Four places may say, nearest first:

    --cxx g++                   this one build
    DZ_CXX=g++                  the environment
    [build] cxx = \"g++\"          the project
    [build.target.linux] cxx    the project, on one platform

With none of them, `dz` uses the first of clang++, g++, c++ that it finds.";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("check") => match build_args(&args) {
            Ok(b) if b.json => cmd_check_json(&b.path, &b.options),
            Ok(b) => cmd_check(&b.path, &b.options),
            Err(code) => code,
        },
        Some("fix") => match build_args(&args) {
            Ok(b) => cmd_fix(&b.path, &b.options, b.dry_run),
            Err(code) => code,
        },
        Some("hir") => match build_args(&args) {
            Ok(b) => cmd_hir(&b.path, &b.options),
            Err(code) => code,
        },
        Some("cpp") => with_mode(&args, Stage::Cpp),
        Some("build") => with_mode(&args, Stage::Build),
        Some("run") => with_mode(&args, Stage::Run),
        Some("test") => cmd_harness(&args, dz_cpp::Mode::Tests),
        Some("bench") => cmd_harness(&args, dz_cpp::Mode::Benches),
        Some("lex") => with_file(&args, cmd_lex),
        Some("parse") => with_file(&args, cmd_parse),
        Some("expand") => with_file(&args, cmd_expand),
        Some("fmt") => cmd_fmt(&args[1..]),
        Some("lsp") => cmd_lsp(),
        Some("explain") => cmd_explain(args.get(1).map(String::as_str)),
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
fn cmd_hir(entry: &str, options: &dz_driver::BuildOptions) -> ExitCode {
    let analysis = analyse_for_build(entry, options);
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

fn cmd_check(entry: &str, options: &dz_driver::BuildOptions) -> ExitCode {
    let analysis = analyse_for_build(entry, options);
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

/// `dz check --format=json`: every diagnostic, warnings included, as one JSON document on
/// stdout and nothing else there — the shape is `dz_diag::render_json`'s. The exit code is
/// still the verdict: failure when there is an error.
fn cmd_check_json(entry: &str, options: &dz_driver::BuildOptions) -> ExitCode {
    let analysis = analyse_for_build(entry, options);
    print!("{}", dz_diag::render_json(&analysis.map, &analysis.diags));
    if analysis.diags.has_errors() {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// `dz fix`: apply every certain fix the diagnostics carry, then report what is left.
///
/// Only a fix marked certain is applied — a suggestion such as "did you mean `count`?" is a
/// guess, and `dz fix` runs unattended (see `dz_diag::Fix`). Edits are byte ranges in the
/// file as it is on disk (invariant 2), applied last first so earlier offsets stay true;
/// one that overlaps an edit already taken waits for the next run, as does anything a fix
/// uncovers, since fixing one mistake can let a later phase see the next.
fn cmd_fix(entry: &str, options: &dz_driver::BuildOptions, dry_run: bool) -> ExitCode {
    let analysis = analyse_for_build(entry, options);
    let map = &analysis.map;
    let mut by_file: std::collections::BTreeMap<FileId, Vec<&dz_diag::Edit>> =
        std::collections::BTreeMap::new();
    let mut applied: Vec<(&dz_diag::Diagnostic, &dz_diag::Fix)> = Vec::new();
    for d in analysis.diags.iter() {
        let Some(fix) = d.fix.as_ref().filter(|f| f.certain) else { continue };
        // A fix whose edits cannot all be taken is not taken at all: half a fix is a
        // different program.
        let clashes = fix.edits.iter().any(|e| {
            by_file.get(&e.file).is_some_and(|taken| {
                taken.iter().any(|t| {
                    e.span.start < t.span.end.max(t.span.start + 1)
                        && t.span.start < e.span.end.max(e.span.start + 1)
                })
            })
        });
        // Only a file that is the project's own, on disk: never the standard library's
        // sources, which the compiler carries.
        let on_disk =
            fix.edits.iter().all(|e| map.try_get(e.file).is_some_and(|f| f.path().is_file()));
        if clashes || !on_disk {
            continue;
        }
        for e in &fix.edits {
            by_file.entry(e.file).or_default().push(e);
        }
        applied.push((d, fix));
    }

    for (d, fix) in &applied {
        let at = d
            .primary_span()
            .and_then(|(f, s)| map.try_get(f).map(|file| (file, file.location(s.start as usize))))
            .map(|(file, loc)| {
                format!("{}:{}:{}", file.name().replace('\\', "/"), loc.line, loc.column)
            })
            .unwrap_or_default();
        let code = d.code.unwrap_or("");
        let would = if dry_run { "would " } else { "" };
        println!("{at}: {code} {would}{}", fix.title);
    }

    let mut failed = false;
    if !dry_run {
        for (file, mut edits) in by_file {
            let source = map.get(file);
            let mut text = source.text().to_owned();
            edits.sort_by_key(|e| std::cmp::Reverse((e.span.start, e.span.end)));
            for e in edits {
                text.replace_range(e.span.range(), &e.text);
            }
            if let Err(e) = std::fs::write(source.path(), text) {
                eprintln!("error: cannot write {}: {e}", source.path().display());
                failed = true;
            }
        }
    }

    let n = applied.len();
    let left = analysis.diags.len() - n;
    println!(
        "{} {n} fix{}; {left} diagnostic{} not fixed{}",
        if dry_run { "would apply" } else { "applied" },
        if n == 1 { "" } else { "es" },
        if left == 1 { "" } else { "s" },
        if n > 0 && !dry_run {
            " — run `dz check` again, since a fix can uncover the next mistake"
        } else {
            ""
        }
    );
    if failed || (left > 0 && analysis.diags.has_errors()) {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
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
    let mut cxx: Option<String> = None;
    let mut build_flags = dz_driver::BuildOptions::default();
    let mut positional = 0;
    let mut rest = args[1..].iter();
    while let Some(a) = rest.next() {
        match a.as_str() {
            "--cxx" => match rest.next() {
                Some(c) => cxx = Some(c.clone()),
                None => {
                    eprintln!("error: `--cxx` needs a compiler\n\n{USAGE}");
                    return ExitCode::FAILURE;
                }
            },
            other if other.starts_with("--cxx=") => cxx = Some(other[6..].to_owned()),
            "--debug" => mode = Some(Optimize::Debug),
            "--release" => mode = Some(Optimize::Release),
            "--shipping" => mode = Some(Optimize::Shipping),
            "--track-memory" => build_flags.track_memory = true,
            "--flag" => match rest.next() {
                Some(f) => build_flags.flags.push(f.clone()),
                None => {
                    eprintln!("error: `--flag` needs a flag name\n\n{USAGE}");
                    return ExitCode::FAILURE;
                }
            },
            other if other.starts_with("--flag=") => build_flags.flags.push(other[7..].to_owned()),
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
    let cxx = cxx.as_deref();

    build_flags.optimize = mode;
    let analysis = analyse_for_build(path, &build_flags);
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
        println!("no {what}s found; write `#[{attr}] fn name() {{ … }}` beside the code it covers");
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
    let exe = match build::compile(&analysis, &out, &root, cxx) {
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

    println!("running {} {what}{}", names.len(), if names.len() == 1 { "" } else { "s" });
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

/// A command's path, build options and `--cxx`, read from its arguments.
struct BuildArgs {
    path: String,
    options: dz_driver::BuildOptions,
    cxx: Option<String>,
    /// `--format=json`: diagnostics as one JSON document on stdout, for a program to read.
    json: bool,
    /// `dz fix --dry-run`: say what would change, and change nothing.
    dry_run: bool,
}

/// Split `run`/`build`/`cpp`/`check`/`hir`'s arguments into a path, a build mode and flags.
///
/// The mode overrides the manifest's `[build] optimize` for one command, so a project can
/// keep `debug` as what it is worked on in while `dz run --release` measures it. Written
/// as a flag rather than a subcommand because it is the same command either way. `check`
/// takes the same options because the mode and the flags decide which `#if` branches are
/// checked at all (spec §12.10).
fn build_args(args: &[String]) -> Result<BuildArgs, ExitCode> {
    let mut out = BuildArgs {
        path: ".".to_owned(),
        options: dz_driver::BuildOptions::default(),
        cxx: None,
        json: false,
        dry_run: false,
    };
    let mut rest = args[1..].iter();
    while let Some(a) = rest.next() {
        match a.as_str() {
            "--format" | "--format=json" | "--format=human" if args[0] == "check" => {
                let value = match a.strip_prefix("--format=") {
                    Some(v) => Some(v),
                    None => rest.next().map(String::as_str),
                };
                match value {
                    Some("json") => out.json = true,
                    Some("human") => out.json = false,
                    _ => {
                        eprintln!("error: `--format` is `human` or `json`\n\n{USAGE}");
                        return Err(ExitCode::FAILURE);
                    }
                }
            }
            "--dry-run" if args[0] == "fix" => out.dry_run = true,
            // `--cxx g++` and `--cxx=g++` both, since a flag taking a value is usually
            // written either way and guessing wrong is a confusing error.
            "--cxx" => match rest.next() {
                Some(c) => out.cxx = Some(c.clone()),
                None => {
                    eprintln!("error: `--cxx` needs a compiler\n\n{USAGE}");
                    return Err(ExitCode::FAILURE);
                }
            },
            other if other.starts_with("--cxx=") => out.cxx = Some(other[6..].to_owned()),
            "--debug" => out.options.optimize = Some(Optimize::Debug),
            "--release" => out.options.optimize = Some(Optimize::Release),
            "--shipping" => out.options.optimize = Some(Optimize::Shipping),
            "--track-memory" => out.options.track_memory = true,
            "--flag" => match rest.next() {
                Some(f) => out.options.flags.push(f.clone()),
                None => {
                    eprintln!("error: `--flag` needs a flag name\n\n{USAGE}");
                    return Err(ExitCode::FAILURE);
                }
            },
            other if other.starts_with("--flag=") => out.options.flags.push(other[7..].to_owned()),
            other if other.starts_with("--") => {
                eprintln!("error: unknown option `{other}`\n\n{USAGE}");
                return Err(ExitCode::FAILURE);
            }
            other => out.path = other.to_owned(),
        }
    }
    Ok(out)
}

fn with_mode(args: &[String], stage: Stage) -> ExitCode {
    match build_args(args) {
        Ok(b) => cmd_pipeline(&b.path, stage, &b.options, b.cxx.as_deref()),
        Err(code) => code,
    }
}

/// Analyse one build: its mode and flags decide which `#if` branches exist (spec §12.10).
fn analyse_for_build(entry: &str, options: &dz_driver::BuildOptions) -> build::Analysis {
    dz_driver::analyse_for(Path::new(entry), &|_| None, dz_driver::Mode::Build, options)
}

/// `dz cpp`, `dz build` and `dz run` share everything up to where they stop.
fn cmd_pipeline(
    entry: &str,
    stage: Stage,
    options: &dz_driver::BuildOptions,
    cxx: Option<&str>,
) -> ExitCode {
    let analysis = analyse_for_build(entry, options);
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

    let exe = match build::compile(&analysis, &out, &root, cxx) {
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

/// `dz explain DZ0401` (or `0401`, `401`): the rule behind a diagnostic, why it exists, and
/// a program that breaks it beside one that does not — every example compiled by a test,
/// so it says what the compiler does. With no code, the list of every code.
fn cmd_explain(code: Option<&str>) -> ExitCode {
    let Some(arg) = code else {
        print!("{}", dz_diag::explain::index());
        return ExitCode::SUCCESS;
    };
    let found = dz_diag::explain::normalise(arg).and_then(|c| dz_diag::explain::explain(&c));
    match found {
        Some(e) => {
            print!("{}", dz_diag::explain::render(&e));
            ExitCode::SUCCESS
        }
        None => {
            eprintln!("error: `{arg}` is not a diagnostic code; `dz explain` lists them all");
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
        eprintln!(
            "
{n} error{} emitted",
            if n == 1 { "" } else { "s" }
        );
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

/// `dz expand file.dz`: the file as resolution sees it, every invocation replaced by its
/// expansion and printed by the formatter (spec §12.6). A name hygiene renamed prints with
/// its `·N`, which is exactly how it differs from the caller's.
fn cmd_expand(path: &str) -> ExitCode {
    let file_path = match std::fs::canonicalize(path) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: cannot read {path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let Some(manifest) =
        file_path.ancestors().skip(1).map(|d| d.join("dazzle.toml")).find(|m| m.is_file())
    else {
        eprintln!("error: {path} is not inside a project: no dazzle.toml above it");
        return ExitCode::FAILURE;
    };
    let analysis = dz_driver::analyse_for(
        &manifest,
        &|_| None,
        dz_driver::Mode::Editor,
        &dz_driver::BuildOptions::default(),
    );
    if !analysis.diags.is_empty() {
        dz_diag::emit_to_stderr(&analysis.map, &analysis.diags);
    }
    let found = analysis.files.values().flatten().find(|pf| {
        std::fs::canonicalize(analysis.map.get(pf.file).path()).is_ok_and(|p| p == file_path)
    });
    let Some(pf) = found else {
        eprintln!("error: {path} is not one of its project's sources");
        return ExitCode::FAILURE;
    };
    let file = analysis.map.get(pf.file);
    let lexed = dz_lex::lex(pf.file, file.text());
    let synthetic = analysis
        .expansions
        .iter()
        .filter(|r| r.file == pf.file)
        .map(|r| r.name_span.start)
        .collect();
    print!("{}", dz_fmt::format_expanded(file, &pf.ast, &lexed.comments, &synthetic));
    if analysis.diags.has_errors() {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
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
