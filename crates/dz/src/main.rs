//! The `dz` command line driver.
//!
//! `lex`, `parse` and `fmt` exist at M1. The full subcommand set is in spec §17.3.

use std::path::Path;
use std::process::ExitCode;

mod build;
mod native;

use dz_span::{FileId, SourceMap};

const USAGE: &str = "\
dz — the Dazzle compiler

usage:
    dz run   [path]             build a project and execute it
    dz build [path]             generate C++ and compile it
    dz cpp   [path]             generate C++ only, no compilation
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
directory. Other subcommands (test, expand) are not implemented yet.";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("check") => cmd_check(args.get(1).map(String::as_str).unwrap_or(".")),
        Some("hir") => cmd_hir(args.get(1).map(String::as_str).unwrap_or(".")),
        Some("cpp") => cmd_pipeline(args.get(1).map(String::as_str).unwrap_or("."), Stage::Cpp),
        Some("build") => cmd_pipeline(args.get(1).map(String::as_str).unwrap_or("."), Stage::Build),
        Some("run") => cmd_pipeline(args.get(1).map(String::as_str).unwrap_or("."), Stage::Run),
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

/// `dz cpp`, `dz build` and `dz run` share everything up to where they stop.
fn cmd_pipeline(entry: &str, stage: Stage) -> ExitCode {
    let analysis = build::analyse(Path::new(entry));
    if !analysis.diags.is_empty() {
        dz_diag::emit_to_stderr(&analysis.map, &analysis.diags);
    }
    if analysis.diags.has_errors() {
        return failure(&analysis.diags);
    }

    let root = project_root(entry);
    let out = match build::emit_cpp(&analysis, &root) {
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
