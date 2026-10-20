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
    /// The *diagnostics* are exactly [`Mode::Build`]'s. A phase is dropped when an earlier
    /// failure made its answers meaningless — everything after a parse error, since the
    /// tree itself is then a guess, and ownership after a resolution error, since it needs
    /// types it can trust. Only the tree, the symbols and the types are kept.
    ///
    /// **Type checking is not dropped for a name that did not resolve.** Such an expression
    /// has type `Ty::Error` and the checker already declines to say anything more about it,
    /// so suppressing the phase wholesale only threw away the diagnostics that had nothing
    /// to do with it — in a file being typed into, which is nearly always one with a name
    /// not yet written, that was most of them.
    Editor,
}

pub struct Analysis {
    pub map: SourceMap,
    /// `None` when the root manifest could not be loaded at all, in which case `diags`
    /// says why and no later phase ran.
    pub ws: Option<Workspace>,
    /// The files as resolution saw them: every macro invocation replaced by its expansion
    /// (spec §12.5), and every `#if` by its active branch.
    pub files: HashMap<ModuleId, Vec<ParsedFile>>,
    /// Every invocation that was expanded, and where its macro is declared.
    pub expansions: Vec<dz_macro::Record>,
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
            expansions: Vec::new(),
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
            // The standard library's text never changes within a process, so its tree is
            // parsed once and copied into each analysis — the editor analyses on every
            // pause in typing, and `std` is most of what a small program parses (§14.4).
            if module.is_prelude() {
                if let Some(ast) = std_tree(path) {
                    let text = module.read_source(path).unwrap_or_default();
                    if let Ok(id) = map.add_bytes(path, &text) {
                        parsed.push(ParsedFile { file: id, ast });
                        continue;
                    }
                }
            }
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

    import_headers(&mut ws, &mut map, &mut diags);

    // Only the active branch of each `#if` goes on to resolution (spec §12.10). Skipped
    // when parsing failed: a broken `#if` has no branches worth choosing between.
    let flags = (!diags.has_errors()).then(|| cfg::resolve_flags(&ws, options, &mut diags));
    if let Some(flags) = &flags {
        for (id, parsed) in files.iter_mut() {
            cfg::strip(&ws, flags, *id, parsed, &mut diags);
        }
    }

    // Macros expand after `#if`, so an invocation in a branch not built is never expanded,
    // and before resolution, which sees only what they expanded to (spec §12.11). What an
    // expansion wrote may hold an `#if` of its own, decided the same way.
    // After a parse error the editor still expands, so what was typed inside an invocation
    // can be answered for, but says nothing more than a build would (see `Mode::Editor`).
    let expansions = if !diags.has_errors() {
        dz_macro::expand(&ws, &mut files, &mut diags)
    } else if mode == Mode::Editor {
        dz_macro::expand(&ws, &mut files, &mut Diagnostics::new())
    } else {
        Vec::new()
    };
    if let Some(flags) = flags.as_ref().filter(|_| !expansions.is_empty()) {
        for (id, parsed) in files.iter_mut() {
            cfg::strip(&ws, flags, *id, parsed, &mut diags);
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
        // The checker runs, and its diagnostics are kept, whenever the program *parsed* —
        // a name that failed to resolve no longer silences the rest of the file.
        //
        // It used to take `resolved_cleanly`, which is `has_errors()` over the whole
        // workspace, so one unresolved name anywhere discarded every type error
        // everywhere: a `dazzle.toml` missing its `std` dependency left `Iterator`
        // unresolved and, with it, a non-exhaustive `switch` on a locally declared enum in
        // another function went unreported. The work was done and thrown away — `hir` and
        // `table` were kept either way, which is why hover went on working.
        //
        // What that blanket gate was for is real but narrower: an expression built from a
        // name that did not resolve has type `Ty::Error`, and anything said about it is
        // noise. That is suppressed where it arises instead — the checker's `is_error()`
        // guards — which leaves the diagnostics that never touched the unresolved name.
        //
        // A *parse* error still suppresses, and must: the tree itself is then a guess.
        if parsed_cleanly || tolerant {
            let checked = dz_check::check(&ws, &r, &files, &map);
            let checked_cleanly = resolved_cleanly && !checked.diagnostics.has_errors();
            if parsed_cleanly {
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

    dz_macro::annotate(&expansions, &map, &mut diags);
    diags.sort_by_position();
    Analysis {
        map,
        ws: Some(ws),
        files,
        expansions,
        resolution,
        hir,
        table,
        mono,
        diags,
        parsed_cleanly,
    }
}

/// The parsed tree of the standard-library file at `pseudo`, from a process-wide cache.
///
/// A tree holds byte offsets and no file ids, so one parse serves every source map it is
/// added to. A file that lexed or parsed with a diagnostic is not cached — those carry the
/// id they were reported against — and falls back to being parsed where it is used, where
/// the mistake is reported as usual (it would be a compiler bug either way).
fn std_tree(pseudo: &Path) -> Option<dz_ast::SourceFileAst> {
    use std::sync::OnceLock;
    static TREES: OnceLock<HashMap<&'static str, dz_ast::SourceFileAst>> = OnceLock::new();
    let trees = TREES.get_or_init(|| {
        let mut out = HashMap::new();
        for (path, text) in dz_project::STD_SOURCES {
            let file = FileId(0);
            let lexed = dz_lex::lex(file, text);
            let parsed = dz_parse::parse(file, text, &lexed.tokens);
            if lexed.diagnostics.is_empty() && parsed.diagnostics.is_empty() {
                out.insert(*path, parsed.ast);
            }
        }
        out
    });
    let key = pseudo.to_string_lossy().replace('\\', "/");
    trees.get(key.as_str()).cloned()
}

/// `rl = { cpp = "raylib" }` (spec §15.1): read the header each such dependency's recipe
/// imports through libclang, with the flags the generated C++ will be compiled with, and
/// add it to the workspace as a module of its own before resolution needs its names. Its
/// files go into the source map, so what points at a declaration points into the header.
fn import_headers(ws: &mut Workspace, map: &mut SourceMap, diags: &mut Diagnostics) {
    use dz_project::cppimport::{import, ImportError, Request};

    let root = ws.module(ws.root);
    // Where the generated C++ is compiled, so a quoted include is searched for from there.
    let from_dir = root.root_dir.join("build").join("cpp");
    let cpp_standard = root.build.cpp_standard.clone();
    let mut added: HashMap<PathBuf, FileId> = HashMap::new();
    let ids: Vec<ModuleId> = ws.modules().map(|m| m.id).collect();

    for id in ids {
        let module = ws.module(id);
        let manifest = module.manifest;
        let wanted = module.cpp_deps.clone();
        for dep in wanted {
            // The recipe is this manifest's, or a dependency's that carries it for its users
            // (spec §15.1): the first one found, nearest first.
            let recipe = std::iter::once(id)
                .chain(ws.module(id).deps.iter().map(|d| d.module))
                .find_map(|m| ws.module(m).cpp.libs.iter().find(|l| l.name == dep.recipe).cloned());
            let Some(recipe) = recipe else {
                diags.push(
                    Diagnostic::error(
                        codes::MANIFEST_BAD_VALUE,
                        format!("no `[cpp.libs.{}]` recipe for `{}`", dep.recipe, dep.alias),
                    )
                    .at(manifest, dep.recipe_span, "no recipe by this name")
                    .with_help(format!(
                        "add `[cpp.libs.{}]` with `import = [\"header.h\"]` (spec §15.1)",
                        dep.recipe
                    )),
                );
                continue;
            };
            let names: Vec<String> = recipe.import.iter().map(|(n, _)| n.clone()).collect();
            let header = match &recipe.import[..] {
                [(_, first), ..] => *first,
                [] => {
                    diags.push(
                        Diagnostic::error(
                            codes::MANIFEST_MISSING_FIELD,
                            format!("recipe `{}` imports no header", recipe.name),
                        )
                        .at(manifest, dep.recipe_span, "nothing here for Dazzle code to see")
                        .with_help(format!(
                            "name the header in `[cpp.libs.{}]`: `import = [\"{}.h\"]` (spec §15.1)",
                            recipe.name, recipe.name
                        )),
                    );
                    continue;
                }
            };
            // A failure names the header clang could not read, when its message says which.
            let span_for = |messages: &[String]| {
                recipe
                    .import
                    .iter()
                    .find(|(n, _)| messages.iter().any(|m| m.contains(n.as_str())))
                    .map_or(header, |(_, s)| *s)
            };
            // The header's span is in whichever manifest holds the recipe.
            let header_file = ws
                .modules()
                .find(|m| {
                    m.cpp
                        .libs
                        .iter()
                        .any(|l| l.name == recipe.name && l.name_span == recipe.name_span)
                })
                .map_or(manifest, |m| m.manifest);
            let (pf_file, header) = (header_file, header);
            {
                let name = names.join("`, `");
                let flags = ws.cpp_flags(id);
                let instantiate = ws.cpp_instantiate(id);
                let req = Request {
                    headers: &names,
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
                                    pf_file,
                                    header,
                                    "one of the files it includes could not be read",
                                ),
                            );
                            continue;
                        }
                        ws.add_foreign(id, &dep, dz_project::Foreign { header: h, files: ids });
                    }
                    Err(ImportError::Unavailable(why)) => diags.push(
                        Diagnostic::error(
                            codes::CPP_UNAVAILABLE,
                            "libclang is not available, so a C or C++ header cannot be read",
                        )
                        .at(pf_file, header, "needs libclang")
                        .with_note(why)
                        .with_help(
                            "install LLVM, or set LIBCLANG_PATH to the directory holding \
                             libclang (spec §15.1)",
                        ),
                    ),
                    Err(ImportError::Failed(messages)) => {
                        let mut d = Diagnostic::error(codes::CPP_HEADER_FAILED, format!("cannot import `{name}`"))
                            .at(pf_file, span_for(&messages), "clang could not read this header")
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
