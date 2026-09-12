//! The build pipeline: source to C++ to a running program (spec §16).

use std::path::{Path, PathBuf};
use std::process::Command;

use dz_diag::{codes, Diagnostic, Diagnostics};
use dz_project::{Optimize, OutputKind};

// Analysis is shared with the language server, so it lives in `dz_driver` (spec §17.4).
pub use dz_driver::{analyse, Analysis};

pub struct BuildOutput {
    pub cpp_dir: PathBuf,
    /// Only the files whose contents actually changed (spec §16.6).
    pub written: Vec<PathBuf>,
}

/// Generate C++ for a project, writing only files whose contents changed (spec §16.6).
pub fn emit_cpp(analysis: &Analysis, root: &Path) -> Result<BuildOutput, Diagnostics> {
    let (Some(ws), Some(resolution), Some(table), Some(hir), Some(mono)) = (
        &analysis.ws,
        &analysis.resolution,
        &analysis.table,
        &analysis.hir,
        &analysis.mono,
    ) else {
        return Err(Diagnostics::new());
    };
    let emitted =
        dz_cpp::emit(ws, resolution, table, hir, mono, &analysis.map, &analysis.files);
    if emitted.diagnostics.has_errors() {
        return Err(emitted.diagnostics);
    }

    let cpp_dir = root.join("build").join("cpp");
    if let Err(e) = std::fs::create_dir_all(&cpp_dir) {
        let mut d = Diagnostics::new();
        d.push(Diagnostic::error(
            codes::CPP_COMPILER_FAILED,
            format!("cannot create {}: {e}", cpp_dir.display()),
        ));
        return Err(d);
    }

    let mut written = Vec::new();
    for file in &emitted.files {
        let path = cpp_dir.join(&file.name);
        // Rewriting an unchanged file would defeat the C++ compiler's own incremental
        // build, which is the only incrementality that exists so far (spec §16.6).
        let unchanged = std::fs::read_to_string(&path)
            .map(|existing| existing == file.contents)
            .unwrap_or(false);
        if !unchanged {
            if let Err(e) = std::fs::write(&path, &file.contents) {
                let mut d = Diagnostics::new();
                d.push(Diagnostic::error(
                    codes::CPP_COMPILER_FAILED,
                    format!("cannot write {}: {e}", path.display()),
                ));
                return Err(d);
            }
            written.push(path.clone());
        }
    }

    Ok(BuildOutput { cpp_dir, written })
}

/// A C++ compiler to drive, honouring `DZ_CXX` before searching the path.
pub fn find_compiler() -> Option<String> {
    if let Ok(cxx) = std::env::var("DZ_CXX") {
        if !cxx.is_empty() {
            return Some(cxx);
        }
    }
    for candidate in ["clang++", "g++", "c++"] {
        let probe = Command::new(candidate).arg("--version").output();
        if matches!(probe, Ok(o) if o.status.success()) {
            return Some(candidate.to_owned());
        }
    }
    None
}

pub fn compile(
    analysis: &Analysis,
    out: &BuildOutput,
    root: &Path,
) -> Result<PathBuf, Diagnostics> {
    let mut diags = Diagnostics::new();
    let Some(cxx) = find_compiler() else {
        diags.push(
            Diagnostic::error(codes::CPP_COMPILER_FAILED, "no C++ compiler found")
                .with_help(
                    "install clang++ or g++, or set DZ_CXX to the compiler to use",
                ),
        );
        return Err(diags);
    };

    let Some(ws) = &analysis.ws else { return Err(diags) };
    let module = ws.module(ws.root);
    if module.build.output != OutputKind::Executable {
        diags.push(Diagnostic::error(
            codes::CPP_COMPILER_FAILED,
            format!(
                "module `{}` has output = \"{}\"; only executables can be built or run yet",
                module.name,
                match module.build.output {
                    OutputKind::StaticLib => "static-lib",
                    OutputKind::SharedLib => "shared-lib",
                    OutputKind::Executable => "executable",
                }
            ),
        ));
        return Err(diags);
    }

    let bin_dir = root.join("build");
    let exe_name = if cfg!(windows) {
        format!("{}.exe", file_stem(&module.name))
    } else {
        file_stem(&module.name)
    };
    let exe = bin_dir.join(exe_name);

    // Bounds checks are not a build setting: indexing an array or slice is safe code, and
    // stays checked in every build (spec §5.3). Unchecked access is `unsafe { xs.ptr[i] }`.
    let opt: &[&str] = match module.build.optimize {
        Optimize::Debug => &["-O0", "-g"],
        Optimize::Release => &["-O2"],
        Optimize::Shipping => &["-O3", "-DNDEBUG"],
    };

    // Vendored libraries first: a failure there is the library's, and should be reported
    // as such before the generated code is even tried (spec §15.1).
    let objects =
        crate::native::build_libraries(ws, &cxx, &bin_dir, opt, &module.build.cpp_standard)?;

    let mut cmd = Command::new(&cxx);
    // spec §16.5 — nothing generated may need exceptions or RTTI.
    cmd.arg(format!("-std={}", module.build.cpp_standard))
        .args(["-fno-exceptions", "-fno-rtti", "-Wall", "-Wextra"])
        .args(opt);
    // Every module's generated C++ is compiled in this one command, so it gets the flags of
    // everything the root reaches (spec §15.1).
    let flags = ws.cpp_flags(ws.root);
    for dir in &flags.include_dirs {
        cmd.arg(format!("-I{}", dir.display()));
    }
    for define in &flags.defines {
        cmd.arg(format!("-D{define}"));
    }

    let mut sources: Vec<PathBuf> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&out.cpp_dir) {
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().is_some_and(|x| x == "cpp") {
                sources.push(p);
            }
        }
    }
    sources.sort();
    for s in &sources {
        cmd.arg(s);
    }
    cmd.args(&objects);
    cmd.arg("-o").arg(&exe);
    let (lib_dirs, link_libs) = ws.link_inputs();
    for dir in &lib_dirs {
        cmd.arg(format!("-L{}", dir.display()));
    }
    for lib in &link_libs {
        cmd.arg(format!("-l{lib}"));
    }

    let output = match cmd.output() {
        Ok(o) => o,
        Err(e) => {
            diags.push(Diagnostic::error(
                codes::CPP_COMPILER_FAILED,
                format!("could not run {cxx}: {e}"),
            ));
            return Err(diags);
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // A *compile* error in generated code is a compiler bug (invariant 7). A *link*
        // error is almost never one: the code was accepted, and what is missing is a
        // definition — most often a vendored library whose implementation nothing compiles.
        // Saying "this is a bug in the Dazzle compiler" for that sends the reader to the
        // wrong place entirely, so the two are told apart.
        let d = if linker_failure(&stderr) {
            Diagnostic::error(
                codes::CPP_COMPILER_FAILED,
                "the linker could not find something the program calls",
            )
            .with_note(format!("using {cxx}"))
            .with_help(
                "the declarations were found but nothing defines them. For a vendored \
                 library, check its `[cpp.libs.<name>]` recipe compiles the right \
                 `sources` — a single-header library needs one source that defines its \
                 `*_IMPLEMENTATION` macro, since a library's `defines` build its own \
                 sources and nothing else — and that `link_libs` names every system \
                 library it needs (spec §15.1)",
            )
        } else {
            Diagnostic::error(
                codes::CPP_COMPILER_FAILED,
                "the C++ compiler rejected the generated code",
            )
            .with_note(format!("using {cxx}"))
            // A C++ error here is a compiler bug, not a user error. `#line` directives
            // mean it still points at the user's `.dz` source (spec §16.3).
            .with_help(
                "this is a bug in the Dazzle compiler: generated code should always \
                 compile. The C++ output is in build/cpp; `dz cpp` regenerates it",
            )
        };
        diags.push(d.with_note(stderr.trim().to_owned()));
        return Err(diags);
    }

    Ok(exe)
}

pub fn run(exe: &Path) -> std::io::Result<std::process::ExitStatus> {
    Command::new(exe).status()
}

fn file_stem(module: &str) -> String {
    module.replace("::", "_")
}

/// Did the C++ compiler get as far as linking, and fail there?
///
/// Each linker says so in its own words: MSVC's `link.exe` reports `LNK2019`/`LNK1120` for
/// an unresolved symbol, the GNU and LLVM linkers "undefined reference" or "undefined
/// symbol", and every driver prints a "linker command failed" line of its own.
fn linker_failure(stderr: &str) -> bool {
    const MARKS: &[&str] = &[
        "LNK2001",
        "LNK2019",
        "LNK1120",
        "undefined reference",
        "undefined symbol",
        "linker command failed",
        "ld returned",
    ];
    MARKS.iter().any(|m| stderr.contains(m))
}
