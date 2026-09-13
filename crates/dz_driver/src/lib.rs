//! The analysis pipeline, from a manifest to checked, instantiated code (spec §17.2).
//!
//! A library of its own because two front ends drive it: the `dz` CLI and the language
//! server (spec §17.4). The server is "the compiler with a different front end", so it must
//! run exactly this function rather than a copy of it that could drift.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use dz_diag::{codes, Diagnostic, Diagnostics};
use dz_project::{ModuleId, Workspace};
use dz_resolve::{ParsedFile, Resolution};
use dz_span::{FileId, SourceMap};

pub mod cfg;
pub use cfg::BuildOptions;

/// How far analysis goes past a phase that failed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// Stop at the first phase with errors: what `dz check` and `dz build` do.
    Build,
    /// Resolve and check whatever parsed, however broken, so an editor can still answer
    /// "what is this?" around a syntax error — which is most of the time while typing.
    ///
    /// The *diagnostics* are exactly [`Mode::Build`]'s: a later phase's are dropped once an
    /// earlier one has failed, because checking a tree whose names did not resolve — or
    /// resolving one that did not parse — produces noise about things the compiler could
    /// never have known. Only the tree, the symbols and the types are kept.
    Editor,
}

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
    /// Every file lexed and parsed without an error. In [`Mode::Editor`] the later phases
    /// run either way; this says whether what they saw was the whole program.
    pub parsed_cleanly: bool,
}

/// Load a project from disk and run every analysis phase that the previous one left viable.
pub fn analyse(entry: &Path) -> Analysis {
    analyse_with(entry, &|_| None, Mode::Build)
}

/// [`analyse`], reading a source file's text from `overlay` whenever it has one.
///
/// This is how an editor's unsaved buffers get checked: the server passes the text it was
/// sent, and every file it has no buffer for comes from disk as usual. Spans are then
/// offsets into the buffer, which is exactly what the editor is showing.
pub fn analyse_with(
    entry: &Path,
    overlay: &dyn Fn(&Path) -> Option<String>,
    mode: Mode,
) -> Analysis {
    analyse_for(entry, overlay, mode, &BuildOptions::default())
}

/// [`analyse_with`] for one build: its mode and flags decide which `#if` branches exist
/// (spec §12.10), so they are an input to analysis rather than something applied after it.
pub fn analyse_for(
    entry: &Path,
    overlay: &dyn Fn(&Path) -> Option<String>,
    mode: Mode,
    options: &BuildOptions,
) -> Analysis {
    let mut map = SourceMap::new();
    let mut diags = Diagnostics::new();

    let Some(mut ws) = dz_project::load(entry, &mut map, &mut diags) else {
        return Analysis {
            map,
            ws: None,
            files: HashMap::new(),
            resolution: None,
            hir: None,
            table: None,
            mono: None,
            diags,
            parsed_cleanly: false,
        };
    };

    // Every module, not just the root: one command builds one program, and a dependency
    // compiled at a different optimisation level is the classic way to get a binary that
    // behaves differently from either setting.
    if let Some(m) = options.optimize {
        for module in ws.modules_mut() {
            module.build.optimize = m;
        }
    }

    let mut files: HashMap<ModuleId, Vec<ParsedFile>> = HashMap::new();
    for module in ws.modules() {
        let mut parsed = Vec::new();
        for path in &module.sources {
            // The prelude is carried by the compiler, so no editor buffer can stand in for it.
            let buffered = if module.is_prelude() { None } else { overlay(path) };
            let bytes = match buffered {
                Some(text) => Ok(text.into_bytes()),
                None => module.read_source(path),
            };
            let bytes = match bytes {
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

    import_std(&mut ws, &mut files, &mut map, &mut diags);
    import_headers(&mut ws, &files, &mut map, &mut diags);

    // Only the active branch of each `#if` goes on to resolution (spec §12.10). Skipped
    // when parsing failed: a broken `#if` has no branches worth choosing between.
    if !diags.has_errors() {
        let flags = cfg::resolve_flags(&ws, options, &mut diags);
        for (id, parsed) in files.iter_mut() {
            cfg::strip(&ws, &flags, *id, parsed, &mut diags);
        }
    }

    // Each flag says whether the phase's diagnostics are still worth showing: true until
    // some earlier phase has reported an error.
    let parsed_cleanly = !diags.has_errors();
    let tolerant = mode == Mode::Editor;
    let mut resolution = None;
    let mut hir = None;
    let mut table = None;
    let mut mono = None;
    if parsed_cleanly || tolerant {
        let r = dz_resolve::resolve(&ws, &files);
        let resolved_cleanly = parsed_cleanly && !r.diagnostics.has_errors();
        if parsed_cleanly {
            diags.extend(r.diagnostics.clone());
        }
        if resolved_cleanly || tolerant {
            let checked = dz_check::check(&ws, &r, &files);
            let checked_cleanly = resolved_cleanly && !checked.diagnostics.has_errors();
            if resolved_cleanly {
                diags.extend(checked.diagnostics);
            }
            // Ownership runs on the typed tree (spec §17.2). Skipped when anything before
            // it failed: a body whose types are unknown moves values of unknown kinds, and
            // every answer this pass gave would be a guess.
            if checked_cleanly {
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
    Analysis { map, ws: Some(ws), files, resolution, hir, table, mono, diags, parsed_cleanly }
}

/// `import std::math` (spec §14.4): the standard library's modules are carried by the
/// compiler, and one becomes part of the workspace — parsed like any module, and a
/// dependency of each module that imports it — only when some file asks for it. Repeated
/// until nothing new is added, since a standard module may import another.
fn import_std(
    ws: &mut Workspace,
    files: &mut HashMap<ModuleId, Vec<ParsedFile>>,
    map: &mut SourceMap,
    diags: &mut Diagnostics,
) {
    use dz_ast::ImportKind;

    let mut pending: Vec<ModuleId> = ws.modules().map(|m| m.id).collect();
    while let Some(id) = pending.pop() {
        let wanted: Vec<String> = files
            .get(&id)
            .into_iter()
            .flatten()
            .flat_map(|pf| &pf.ast.imports)
            .filter_map(|i| match &i.kind {
                ImportKind::Module { path, .. } | ImportKind::Selective { path, .. } => Some(
                    path.segments
                        .iter()
                        .map(|s| s.name.name.as_str())
                        .collect::<Vec<_>>()
                        .join("::"),
                ),
                _ => None,
            })
            .filter(|name| name.starts_with(&format!("{}::", dz_project::STD_ROOT)))
            .collect();
        for name in wanted {
            let Some((std_id, fresh)) = ws.add_std(id, &name, map) else { continue };
            if !fresh {
                continue;
            }
            // One source, already in the map as the module's "manifest": parsed under that
            // same id, so the file has one identity rather than two.
            let file = ws.module(std_id).manifest;
            let mut parsed = Vec::new();
            {
                let text = map.get(file).text().to_owned();
                let lexed = dz_lex::lex(file, &text);
                let parsed_file = dz_parse::parse(file, &text, &lexed.tokens);
                diags.extend(lexed.diagnostics);
                diags.extend(parsed_file.diagnostics);
                parsed.push(ParsedFile { file, ast: parsed_file.ast });
            }
            files.insert(std_id, parsed);
            pending.push(std_id);
        }
    }
}

/// `import cpp` (spec §15.1): read each imported header through libclang, with the flags
/// the generated C++ will be compiled with, and add it to the workspace as a module of its
/// own before resolution needs its names. Its files go into the source map, so what points
/// at a declaration points into the header.
fn import_headers(
    ws: &mut Workspace,
    files: &HashMap<ModuleId, Vec<ParsedFile>>,
    map: &mut SourceMap,
    diags: &mut Diagnostics,
) {
    use dz_ast::ImportKind;
    use dz_project::cppimport::{import, ImportError, Request};

    let root = ws.module(ws.root);
    // Where the generated C++ is compiled, so a quoted include is searched for from there.
    let from_dir = root.root_dir.join("build").join("cpp");
    let cpp_standard = root.build.cpp_standard.clone();
    let mut added: HashMap<PathBuf, FileId> = HashMap::new();
    let ids: Vec<ModuleId> = ws.modules().map(|m| m.id).collect();

    for id in ids {
        let Some(parsed) = files.get(&id) else { continue };
        let mut done: Vec<&str> = Vec::new();
        for pf in parsed {
            for import_line in &pf.ast.imports {
                let ImportKind::Cpp { header, name, .. } = &import_line.kind else { continue };
                if name.is_empty() || done.contains(&name.as_str()) {
                    continue;
                }
                done.push(name);
                let flags = ws.cpp_flags(id);
                let instantiate = ws.cpp_instantiate(id);
                let req = Request {
                    header: name,
                    include_dirs: &flags.include_dirs,
                    defines: &flags.defines,
                    cpp_standard: &cpp_standard,
                    from_dir: &from_dir,
                    instantiate: &instantiate,
                };
                match import(&req) {
                    Ok(h) => {
                        let mut ids = Vec::new();
                        for p in &h.files {
                            if let Some(f) = added.get(p) {
                                ids.push(*f);
                                continue;
                            }
                            match std::fs::read(p).ok().and_then(|b| map.add_bytes(p, &b).ok()) {
                                Some(f) => {
                                    added.insert(p.clone(), f);
                                    ids.push(f);
                                }
                                None => break,
                            }
                        }
                        if ids.len() != h.files.len() {
                            diags.push(
                                Diagnostic::error(
                                    codes::CPP_HEADER_FAILED,
                                    format!("cannot import `{name}`"),
                                )
                                .at(
                                    pf.file,
                                    *header,
                                    "one of the files it includes could not be read",
                                ),
                            );
                            continue;
                        }
                        ws.add_foreign(id, name, dz_project::Foreign { header: h, files: ids });
                    }
                    Err(ImportError::Unavailable(why)) => diags.push(
                        Diagnostic::error(
                            codes::CPP_UNAVAILABLE,
                            "libclang is not available, so `import cpp` cannot read headers",
                        )
                        .at(pf.file, *header, "needs libclang")
                        .with_note(why)
                        .with_help(
                            "install LLVM, or set LIBCLANG_PATH to the directory holding \
                             libclang (spec §15.1)",
                        ),
                    ),
                    Err(ImportError::Failed(messages)) => {
                        let mut d = Diagnostic::error(codes::CPP_HEADER_FAILED, format!("cannot import `{name}`"))
                            .at(pf.file, *header, "clang could not read this header")
                            .with_help(
                                "headers are found through the `include_dirs` of this module's \
                                 `[cpp]` and `[cpp.libs]` tables, and its dependencies' (spec §15.1)",
                            );
                        for m in messages.iter().take(5) {
                            d = d.with_note(m.clone());
                        }
                        diags.push(d);
                    }
                }
            }
        }
    }
}
