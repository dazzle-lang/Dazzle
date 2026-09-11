//! The analysis pipeline, from a manifest to checked, instantiated code (spec §17.2).
//!
//! A library of its own because two front ends drive it: the `dz` CLI and the language
//! server (spec §17.4). The server is "the compiler with a different front end", so it must
//! run exactly this function rather than a copy of it that could drift.

use std::collections::HashMap;
use std::path::Path;

use dz_diag::{codes, Diagnostic, Diagnostics};
use dz_project::{ModuleId, Workspace};
use dz_resolve::{ParsedFile, Resolution};
use dz_span::SourceMap;

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
            parsed_cleanly: false,
        };
    };

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
