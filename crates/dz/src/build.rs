//! The build pipeline: source to C++ to a running program (spec §16).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use dz_diag::{codes, Diagnostic, Diagnostics};
use dz_project::{ModuleId, Optimize, OutputKind, Workspace};
use dz_resolve::{ParsedFile, Resolution};
use dz_span::SourceMap;

pub struct Analysis {
    pub map: SourceMap,
    /// `None` when the root manifest could not be loaded at all, in which case `diags`
    /// says why and no later phase ran.
    pub ws: Option<Workspace>,
    pub files: HashMap<ModuleId, Vec<ParsedFile>>,
    pub resolution: Option<Resolution>,
    /// The typed tree, when checking ran (spec §17.1). `dz hir` prints it, and from M5 on
    /// flow analysis and codegen consume it.
    pub hir: Option<dz_hir::Hir>,
    /// Signatures, which the HIR does not carry: it has bodies but no items, so anything
    /// asking what a struct's fields are called goes here (spec §17.1).
    pub table: Option<dz_check::TypeTable>,
    /// Which generic instantiations each module needs, with a concrete copy of every body
    /// (spec §8.4). `None` when checking failed, since there is nothing sound to instantiate.
    pub mono: Option<dz_mono::Mono>,
    pub diags: Diagnostics,
}

/// Load a project and run every analysis phase that the previous one left viable.
///
/// Each phase is skipped when an earlier one failed, because checking a tree whose names
/// did not resolve — or resolving one that did not parse — produces noise about things the
/// compiler could never have known.
pub fn analyse(entry: &Path) -> Analysis {
    let mut map = SourceMap::new();
    let mut diags = Diagnostics::new();

    let Some(ws) = dz_project::load(entry, &mut map, &mut diags) else {
        return Analysis {
            map,
            ws: None,
            files: HashMap::new(),
            resolution: None,
            hir: None,
            table: None,
            mono: None,
            diags,
        };
    };

    let mut files: HashMap<ModuleId, Vec<ParsedFile>> = HashMap::new();
    for module in ws.modules() {
        let mut parsed = Vec::new();
        for path in &module.sources {
            let bytes = match module.read_source(path) {
                Ok(b) => b,
                Err(e) => {
                    diags.push(Diagnostic::error(
                        codes::MANIFEST_MISSING,
                        format!("cannot read {}: {e}", path.display()),
                    ));
                    continue;
                }
            };
            let id = match map.add_bytes(path, &bytes) {
                Ok(id) => id,
                Err(e) => {
                    diags.push(Diagnostic::error(
                        codes::BAD_ENCODING,
                        format!("{}: {e}", path.display()),
                    ));
                    continue;
                }
            };
            let text = map.get(id).text().to_owned();
            let lexed = dz_lex::lex(id, &text);
            let parsed_file = dz_parse::parse(id, &text, &lexed.tokens);
            diags.extend(lexed.diagnostics);
            diags.extend(parsed_file.diagnostics);
            parsed.push(ParsedFile { file: id, ast: parsed_file.ast });
        }
        files.insert(module.id, parsed);
    }

    let mut resolution = None;
    let mut hir = None;
    let mut table = None;
    let mut mono = None;
    if !diags.has_errors() {
        let r = dz_resolve::resolve(&ws, &files);
        let failed = r.diagnostics.has_errors();
        diags.extend(r.diagnostics.clone());
        if !failed {
            let checked = dz_check::check(&ws, &r, &files);
            let type_errors = checked.diagnostics.has_errors();
            diags.extend(checked.diagnostics);
            // Ownership runs on the typed tree (spec §17.2). Skipped when checking
            // failed: a body whose types are unknown moves values of unknown kinds, and
            // every answer this pass gave would be a guess.
            if !type_errors {
                diags.extend(dz_flow::check(&checked.hir, &checked.table, &r));
                // Instantiation runs here rather than at codegen so that `dz check`
                // reports a runaway instantiation (DZ0536) too; it is a property of the
                // program, not of the C++ (spec §8.4).
                let m = dz_mono::monomorphize(&r, &checked.table, &checked.hir);
                diags.extend(m.diagnostics.clone());
                mono = Some(m);
            }
            hir = Some(checked.hir);
            table = Some(checked.table);
        }
        resolution = Some(r);
    }

    diags.sort_by_position();
    Analysis { map, ws: Some(ws), files, resolution, hir, table, mono, diags }
}

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

    let mut cmd = Command::new(&cxx);
    // spec §16.5 — nothing generated may need exceptions or RTTI.
    cmd.arg(format!("-std={}", module.build.cpp_standard))
        .args(["-fno-exceptions", "-fno-rtti", "-Wall", "-Wextra"]);
    // Bounds checks are not a build setting: indexing an array or slice is safe code, and
    // stays checked in every build (spec §5.3). Unchecked access is `unsafe { xs.ptr[i] }`.
    match module.build.optimize {
        Optimize::Debug => {
            cmd.args(["-O0", "-g"]);
        }
        Optimize::Release => {
            cmd.args(["-O2"]);
        }
        Optimize::Shipping => {
            cmd.args(["-O3", "-DNDEBUG"]);
        }
    }
    for dir in &module.cpp.include_dirs {
        cmd.arg(format!("-I{dir}"));
    }
    for define in &module.cpp.defines {
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
    cmd.arg("-o").arg(&exe);
    for lib in &module.cpp.link_libs {
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
        diags.push(
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
            .with_note(stderr.trim().to_owned()),
        );
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
