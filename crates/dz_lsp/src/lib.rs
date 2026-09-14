//! The Dazzle language server (spec §17.4): the compiler with a different front end.
//!
//! Built on `lsp-server` rather than the `tower-lsp` §17.4 first named. The compiler is
//! single-threaded by construction — `Ty` holds `Rc`s — so an [`Analysis`] cannot live in
//! the `Send + Sync` state tower-lsp hands its async handlers. A synchronous loop owns
//! everything instead, which also makes "the query sees every edit sent before it" true by
//! construction rather than by locking.
//!
//! A project is re-checked once the editor has been quiet for [`SETTLE`], or at once when a
//! query needs it, with unsaved buffers standing in for the files on disk. Analysis runs in
//! [`Mode::Editor`]: past a syntax error the tree is still resolved and typed, so queries
//! keep working, while the diagnostics stay those of the first phase that failed.

mod convert;
pub mod ide;

use std::collections::{HashMap, HashSet};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use lsp_server::{Connection, ErrorCode, Message, Notification, Request, Response};
use lsp_types::notification::{self as notif, Notification as _};
use lsp_types::request::{self as req, Request as _};
use lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, CodeActionParams,
    CodeActionProviderCapability, CodeActionResponse, CompletionItem, CompletionItemKind,
    CompletionOptions, CompletionParams, CompletionResponse, DiagnosticRelatedInformation,
    DiagnosticSeverity, DidChangeTextDocumentParams, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, DidSaveTextDocumentParams, DocumentFormattingParams,
    DocumentHighlight, DocumentHighlightParams, DocumentSymbol, DocumentSymbolParams,
    DocumentSymbolResponse, Documentation, GotoDefinitionParams, GotoDefinitionResponse, Hover,
    HoverContents, HoverParams, HoverProviderCapability, InlayHint, InlayHintKind, InlayHintLabel,
    InlayHintParams, Location, LogMessageParams, MarkupContent, MarkupKind, MessageType,
    NumberOrString, OneOf, ParameterInformation, ParameterLabel, Position, PrepareRenameResponse,
    PublishDiagnosticsParams, Range, ReferenceParams, RenameOptions, RenameParams, SaveOptions,
    SemanticToken, SemanticTokenModifier, SemanticTokenType, SemanticTokens,
    SemanticTokensFullOptions, SemanticTokensLegend, SemanticTokensOptions, SemanticTokensParams,
    SemanticTokensResult, SemanticTokensServerCapabilities, ServerCapabilities, SignatureHelp,
    SignatureHelpOptions, SignatureHelpParams, SignatureInformation, SymbolInformation,
    TextDocumentPositionParams, TextDocumentSyncCapability, TextDocumentSyncKind,
    TextDocumentSyncOptions, TextDocumentSyncSaveOptions, TextEdit, Url, WorkspaceEdit,
    WorkspaceSymbolParams, WorkspaceSymbolResponse,
};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;

use dz_driver::{Analysis, Mode};
use dz_span::{FileId, SourceFile, SourceMap, Span};

pub use convert::path_key;

type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// How long the editor must be quiet before a project is re-checked. Short enough to feel
/// immediate, long enough that a burst of keystrokes costs one analysis rather than one each.
const SETTLE: Duration = Duration::from_millis(150);

/// The protocol's "the request was valid but could not be carried out" (LSP 3.17).
const REQUEST_FAILED: i32 = -32803;

/// Dazzle's own request for the links in a `dazzle.toml`, which is not a document the server
/// is told about: the VS Code extension asks for them from a link provider of its own.
const MANIFEST_LINKS: &str = "dazzle/manifestLinks";

#[derive(serde::Deserialize)]
struct ManifestLinksParams {
    uri: Url,
}

#[derive(serde::Serialize)]
struct ManifestLink {
    range: Range,
    target: Url,
    tooltip: String,
}

/// Serve the protocol on stdin and stdout until the client says to stop.
pub fn run() -> Result<(), BoxError> {
    let (connection, io_threads) = Connection::stdio();
    connection.initialize(serde_json::to_value(capabilities())?)?;
    Server::new(connection).main_loop()?;
    io_threads.join()?;
    Ok(())
}

fn capabilities() -> ServerCapabilities {
    ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Options(TextDocumentSyncOptions {
            open_close: Some(true),
            // Whole buffers. Files are small, and analysis re-reads the project anyway.
            change: Some(TextDocumentSyncKind::FULL),
            save: Some(TextDocumentSyncSaveOptions::SaveOptions(SaveOptions {
                include_text: Some(false),
            })),
            ..Default::default()
        })),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        definition_provider: Some(OneOf::Left(true)),
        references_provider: Some(OneOf::Left(true)),
        document_highlight_provider: Some(OneOf::Left(true)),
        rename_provider: Some(OneOf::Right(RenameOptions {
            prepare_provider: Some(true),
            work_done_progress_options: Default::default(),
        })),
        document_symbol_provider: Some(OneOf::Left(true)),
        workspace_symbol_provider: Some(OneOf::Left(true)),
        document_formatting_provider: Some(OneOf::Left(true)),
        // Quick fixes (`ctrl+.`). What is offered comes from the diagnostics themselves,
        // so this needs no options beyond being on (spec §17.4).
        code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
        inlay_hint_provider: Some(OneOf::Left(true)),
        semantic_tokens_provider: Some(SemanticTokensServerCapabilities::SemanticTokensOptions(
            SemanticTokensOptions {
                work_done_progress_options: Default::default(),
                legend: SemanticTokensLegend {
                    token_types: ide::TokenClass::ALL.iter().map(|c| token_type(*c)).collect(),
                    token_modifiers: vec![
                        SemanticTokenModifier::DECLARATION,
                        SemanticTokenModifier::READONLY,
                    ],
                },
                range: None,
                full: Some(SemanticTokensFullOptions::Bool(true)),
            },
        )),
        signature_help_provider: Some(SignatureHelpOptions {
            trigger_characters: Some(vec!["(".to_owned(), ",".to_owned()]),
            retrigger_characters: None,
            work_done_progress_options: Default::default(),
        }),
        completion_provider: Some(CompletionOptions {
            trigger_characters: Some(vec![".".to_owned(), ":".to_owned()]),
            ..Default::default()
        }),
        // Implicit-drop hints (spec §17.4) are still owed by M9 (deferral register).
        ..Default::default()
    }
}

/// An open editor buffer.
struct Doc {
    uri: Url,
    path: PathBuf,
    text: String,
    /// The nearest `dazzle.toml` above the file, if any.
    manifest: Option<PathBuf>,
}

struct Project {
    manifest: PathBuf,
    /// Something it reads has changed since `current` was computed.
    dirty: bool,
    current: Option<Rc<Analysis>>,
    /// The latest analysis in which everything parsed. Completion falls back on it when the
    /// line being typed has broken the code around it.
    clean: Option<Rc<Analysis>>,
    /// Every document diagnostics were last published for, so they can be cleared.
    published: HashSet<Url>,
}

impl Project {
    fn new(manifest: PathBuf) -> Project {
        Project { manifest, dirty: true, current: None, clean: None, published: HashSet::new() }
    }
}

struct Server {
    conn: Connection,
    /// Open buffers, by [`path_key`].
    docs: HashMap<String, Doc>,
    /// Projects, by the [`path_key`] of their manifest.
    projects: HashMap<String, Project>,
    /// Open buffers belonging to no project whose syntax check is stale.
    loose: HashSet<String>,
}

type Answer = Result<Value, (i32, String)>;

impl Server {
    fn new(conn: Connection) -> Server {
        Server { conn, docs: HashMap::new(), projects: HashMap::new(), loose: HashSet::new() }
    }

    fn main_loop(mut self) -> Result<(), BoxError> {
        loop {
            let pending = !self.loose.is_empty() || self.projects.values().any(|p| p.dirty);
            let msg = if pending {
                match self.conn.receiver.recv_timeout(SETTLE) {
                    Ok(m) => m,
                    Err(e) if e.is_timeout() => {
                        self.flush();
                        continue;
                    }
                    Err(_) => return Ok(()),
                }
            } else {
                match self.conn.receiver.recv() {
                    Ok(m) => m,
                    Err(_) => return Ok(()),
                }
            };
            match msg {
                Message::Request(r) => {
                    if self.conn.handle_shutdown(&r)? {
                        return Ok(());
                    }
                    self.on_request(r);
                }
                Message::Notification(n) => self.on_notification(n),
                Message::Response(_) => {}
            }
        }
    }

    // --- documents ---------------------------------------------------------------------

    fn on_notification(&mut self, n: Notification) {
        match n.method.as_str() {
            notif::DidOpenTextDocument::METHOD => {
                if let Ok(p) = serde_json::from_value::<DidOpenTextDocumentParams>(n.params) {
                    self.open(p.text_document.uri, p.text_document.text);
                }
            }
            notif::DidChangeTextDocument::METHOD => {
                if let Ok(p) = serde_json::from_value::<DidChangeTextDocumentParams>(n.params) {
                    // Full sync: the last change is the whole buffer.
                    if let Some(change) = p.content_changes.into_iter().last() {
                        self.change(&p.text_document.uri, change.text);
                    }
                }
            }
            notif::DidSaveTextDocument::METHOD => {
                if let Ok(p) = serde_json::from_value::<DidSaveTextDocumentParams>(n.params) {
                    if let Some(key) = key_of(&p.text_document.uri) {
                        self.mark(&key);
                    }
                }
            }
            notif::DidCloseTextDocument::METHOD => {
                if let Ok(p) = serde_json::from_value::<DidCloseTextDocumentParams>(n.params) {
                    self.close(&p.text_document.uri);
                }
            }
            notif::DidChangeWatchedFiles::METHOD => self.files_changed(),
            _ => {}
        }
    }

    fn open(&mut self, uri: Url, text: String) {
        let Ok(path) = uri.to_file_path() else { return };
        let key = path_key(&path);
        let manifest = find_manifest(&path);
        self.docs.insert(key.clone(), Doc { uri, path, text, manifest });
        self.mark(&key);
    }

    fn change(&mut self, uri: &Url, text: String) {
        let Some(key) = key_of(uri) else { return };
        if let Some(doc) = self.docs.get_mut(&key) {
            doc.text = text;
        }
        self.mark(&key);
    }

    fn close(&mut self, uri: &Url) {
        let Some(key) = key_of(uri) else { return };
        let Some(doc) = self.docs.remove(&key) else { return };
        match doc.manifest {
            // The file on disk takes over from the buffer, which may say something else.
            Some(m) => self.mark_project(&m),
            None => {
                self.loose.remove(&key);
                self.publish(doc.uri, Vec::new());
            }
        }
    }

    /// A `.dz` file or a manifest was created, changed or deleted. Which project a buffer
    /// belongs to may have changed with it, and any project's file list may have.
    fn files_changed(&mut self) {
        let keys: Vec<String> = self.docs.keys().cloned().collect();
        for key in keys {
            if let Some(doc) = self.docs.get_mut(&key) {
                doc.manifest = find_manifest(&doc.path);
            }
            self.mark(&key);
        }
        for p in self.projects.values_mut() {
            p.dirty = true;
        }
    }

    fn mark(&mut self, key: &str) {
        let Some(doc) = self.docs.get(key) else { return };
        match doc.manifest.clone() {
            Some(m) => self.mark_project(&m),
            None => {
                self.loose.insert(key.to_owned());
            }
        }
    }

    fn mark_project(&mut self, manifest: &Path) {
        self.projects
            .entry(path_key(manifest))
            .or_insert_with(|| Project::new(manifest.to_owned()))
            .dirty = true;
    }

    // --- analysis ----------------------------------------------------------------------

    /// Re-check everything stale and publish what changed.
    fn flush(&mut self) {
        let dirty: Vec<String> =
            self.projects.iter().filter(|(_, p)| p.dirty).map(|(k, _)| k.clone()).collect();
        for key in dirty {
            self.analyse(&key);
        }
        for key in std::mem::take(&mut self.loose) {
            if let Some(doc) = self.docs.get(&key) {
                let (uri, diags) = (doc.uri.clone(), syntax_diagnostics(doc));
                self.publish(uri, diags);
            }
        }
    }

    /// Re-check a whole project and publish what changed. Whole, because there is no
    /// incremental analysis yet; §17.2's per-module cache is owed by M9 (deferral register).
    fn analyse(&mut self, key: &str) {
        let Some(project) = self.projects.get(key) else { return };
        let manifest = project.manifest.clone();

        let docs = &self.docs;
        let overlay = |p: &Path| docs.get(&path_key(p)).map(|d| d.text.clone());
        // A compiler panic is a compiler bug, but it must not take the editor's server down
        // with it: the next edit may well avoid whatever triggered it. Editor mode checks
        // trees with errors in them, the least-travelled path through the checker, so when it
        // panics build mode still gets the diagnostics out.
        let run = |mode| {
            catch_unwind(AssertUnwindSafe(|| dz_driver::analyse_with(&manifest, &overlay, mode)))
        };
        let analysis = match run(Mode::Editor) {
            Ok(a) => Rc::new(a),
            Err(_) => {
                self.log(format!(
                    "the compiler panicked checking {} around an error; this is a compiler bug",
                    manifest.display()
                ));
                match run(Mode::Build) {
                    Ok(a) => Rc::new(a),
                    Err(_) => {
                        if let Some(p) = self.projects.get_mut(key) {
                            p.dirty = false;
                        }
                        return;
                    }
                }
            }
        };

        let mut by_uri: HashMap<Url, Vec<lsp_types::Diagnostic>> = HashMap::new();
        // Every file gets an entry, even an empty one, so that fixing an error clears it.
        for f in analysis.files.values().flatten() {
            if let Some(uri) = self.uri_for(analysis.map.get(f.file).path()) {
                by_uri.entry(uri).or_default();
            }
        }
        let fallback = self.uri_for(&manifest);
        for d in analysis.diags.iter() {
            let converted = to_lsp(&analysis.map, d, &|p| self.uri_for(p), fallback.as_ref());
            if let Some((uri, diag)) = converted {
                by_uri.entry(uri).or_default().push(diag);
            }
        }
        // An open file next to the manifest that its `sources` do not list is not part of
        // the project, but it still deserves a syntax check.
        for doc in self.docs.values() {
            let ours = doc.manifest.as_deref().map(path_key).as_deref() == Some(key);
            if ours && file_in(&analysis, &path_key(&doc.path)).is_none() {
                by_uri.insert(doc.uri.clone(), syntax_diagnostics(doc));
            }
        }

        let Some(project) = self.projects.get_mut(key) else { return };
        let stale: Vec<Url> =
            project.published.iter().filter(|u| !by_uri.contains_key(*u)).cloned().collect();
        project.published = by_uri.keys().cloned().collect();
        project.dirty = false;
        if analysis.parsed_cleanly {
            project.clean = Some(analysis.clone());
        }
        project.current = Some(analysis);

        for uri in stale {
            self.publish(uri, Vec::new());
        }
        for (uri, diags) in by_uri {
            self.publish(uri, diags);
        }
    }

    fn project_of(&self, key: &str) -> Option<&Project> {
        let manifest = self.docs.get(key)?.manifest.as_ref()?;
        self.projects.get(&path_key(manifest))
    }

    /// The up-to-date analysis of the project a document belongs to, and the document's
    /// file in it. Analyses now rather than waiting for the editor to settle: a query must
    /// answer about the text as it is.
    fn analysis_for(&mut self, uri: &Url) -> Option<(Rc<Analysis>, FileId)> {
        let key = key_of(uri)?;
        // A copy of the prelude or of a standard module belongs to no project and may be part
        // of any, so any analysis that has the file answers for it — reached by going to a
        // definition, which analysed a project first.
        if embedded_for_key(&key).is_some() {
            return self.projects.values().find_map(|p| {
                let analysis = p.current.clone()?;
                let file = file_in(&analysis, &key)?;
                Some((analysis, file))
            });
        }
        let manifest = match self.docs.get(&key) {
            Some(d) => d.manifest.clone(),
            None => find_manifest(&uri.to_file_path().ok()?),
        }?;
        let pk = path_key(&manifest);
        if self.projects.get(&pk).map_or(true, |p| p.dirty) {
            self.mark_project(&manifest);
            self.analyse(&pk);
        }
        let analysis = self.projects.get(&pk)?.current.clone()?;
        let file = file_in(&analysis, &key)?;
        Some((analysis, file))
    }

    /// The editor's text for a document, or the file on disk when it is not open.
    fn text_of(&self, uri: &Url) -> Option<(PathBuf, String)> {
        let path = uri.to_file_path().ok()?;
        match self.docs.get(&path_key(&path)) {
            Some(doc) => Some((path, doc.text.clone())),
            None => {
                let text = std::fs::read_to_string(&path).ok()?;
                Some((path, text))
            }
        }
    }

    /// The URI to report a file under: the editor's own spelling when it has the file open,
    /// so a diagnostic lands on the buffer rather than on a differently-cased twin.
    fn uri_for(&self, path: &Path) -> Option<Url> {
        // A pseudo-path (`<std>/text/string.dz`) names nothing on disk; it is
        // shown from a copy that does.
        if path.to_string_lossy().starts_with('<') {
            return Url::from_file_path(embedded_copy(&path.to_string_lossy().replace('\\', "/"))?)
                .ok();
        }
        if let Some(doc) = self.docs.get(&path_key(path)) {
            return Some(doc.uri.clone());
        }
        Url::from_file_path(path).ok()
    }

    fn locations(&self, a: &Analysis, spans: Vec<(FileId, Span)>) -> Vec<Location> {
        spans
            .into_iter()
            .filter_map(|(f, s)| {
                let source = a.map.get(f);
                Some(Location::new(self.uri_for(source.path())?, convert::range(source, s)))
            })
            .collect()
    }

    fn publish(&self, uri: Url, diagnostics: Vec<lsp_types::Diagnostic>) {
        let params = PublishDiagnosticsParams::new(uri, diagnostics, None);
        self.notify(notif::PublishDiagnostics::METHOD, params);
    }

    fn log(&self, message: String) {
        eprintln!("dz lsp: {message}");
        self.notify(
            notif::LogMessage::METHOD,
            LogMessageParams { typ: MessageType::ERROR, message },
        );
    }

    fn notify(&self, method: &str, params: impl Serialize) {
        let n = Notification::new(method.to_owned(), params);
        let _ = self.conn.sender.send(Message::Notification(n));
    }

    // --- queries -----------------------------------------------------------------------

    fn on_request(&mut self, r: Request) {
        let id = r.id.clone();
        let method = r.method.clone();
        let response = match catch_unwind(AssertUnwindSafe(|| self.dispatch(r))) {
            Ok(Some(Ok(value))) => Response::new_ok(id, value),
            Ok(Some(Err((code, message)))) => Response::new_err(id, code, message),
            Ok(None) => Response::new_err(
                id,
                ErrorCode::MethodNotFound as i32,
                format!("`{method}` is not supported"),
            ),
            Err(_) => Response::new_err(
                id,
                ErrorCode::InternalError as i32,
                format!(
                    "the language server panicked answering `{method}`; this is a compiler bug"
                ),
            ),
        };
        let _ = self.conn.sender.send(Message::Response(response));
    }

    fn dispatch(&mut self, r: Request) -> Option<Answer> {
        /// Decode the parameters, run the handler, encode its answer. A handler's `Err` is
        /// a message for the user — "cannot rename a keyword" — not a protocol failure.
        fn call<P: DeserializeOwned, R: Serialize>(
            params: Value,
            f: impl FnOnce(P) -> Result<R, String>,
        ) -> Answer {
            let p = serde_json::from_value(params)
                .map_err(|e| (ErrorCode::InvalidParams as i32, e.to_string()))?;
            let answer = f(p).map_err(|m| (REQUEST_FAILED, m))?;
            serde_json::to_value(answer)
                .map_err(|e| (ErrorCode::InternalError as i32, e.to_string()))
        }
        let params = r.params;
        Some(match r.method.as_str() {
            req::HoverRequest::METHOD => call(params, |p| Ok(self.hover(p))),
            req::GotoDefinition::METHOD => call(params, |p| Ok(self.definition(p))),
            req::References::METHOD => call(params, |p| Ok(self.references(p))),
            req::DocumentHighlightRequest::METHOD => call(params, |p| Ok(self.highlight(p))),
            req::PrepareRenameRequest::METHOD => call(params, |p| self.prepare_rename(p)),
            req::Rename::METHOD => call(params, |p| self.rename(p)),
            req::DocumentSymbolRequest::METHOD => call(params, |p| Ok(self.symbols(p))),
            req::WorkspaceSymbolRequest::METHOD => call(params, |p| Ok(self.workspace_symbols(p))),
            req::Formatting::METHOD => call(params, |p| Ok(self.format(p))),
            req::Completion::METHOD => call(params, |p| Ok(self.complete(p))),
            req::SignatureHelpRequest::METHOD => call(params, |p| Ok(self.signature_help(p))),
            req::InlayHintRequest::METHOD => call(params, |p| Ok(self.inlay_hints(p))),
            req::CodeActionRequest::METHOD => call(params, |p| Ok(self.code_actions(p))),
            req::SemanticTokensFullRequest::METHOD => call(params, |p| Ok(self.semantic_tokens(p))),
            MANIFEST_LINKS => call(params, |p| Ok(self.manifest_links(p))),
            _ => return None,
        })
    }

    fn hover(&mut self, p: HoverParams) -> Option<Hover> {
        let at = p.text_document_position_params;
        let (analysis, file) = self.analysis_for(&at.text_document.uri)?;
        let source = analysis.map.get(file);
        let info = ide::info_at(&analysis, file, convert::offset(source, at.position))?;
        Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: info.hover?,
            }),
            range: Some(convert::range(source, info.span)),
        })
    }

    fn definition(&mut self, p: GotoDefinitionParams) -> Option<GotoDefinitionResponse> {
        let at = p.text_document_position_params;
        let (analysis, file) = self.analysis_for(&at.text_document.uri)?;
        let source = analysis.map.get(file);
        let info = ide::info_at(&analysis, file, convert::offset(source, at.position))?;
        let location = self.locations(&analysis, vec![info.definition?]).pop()?;
        Some(GotoDefinitionResponse::Scalar(location))
    }

    fn references(&mut self, p: ReferenceParams) -> Option<Vec<Location>> {
        let at = p.text_document_position;
        let (analysis, file) = self.analysis_for(&at.text_document.uri)?;
        let offset = convert::offset(analysis.map.get(file), at.position);
        let spans = ide::references(&analysis, file, offset, p.context.include_declaration);
        Some(self.locations(&analysis, spans))
    }

    fn highlight(&mut self, p: DocumentHighlightParams) -> Option<Vec<DocumentHighlight>> {
        let at = p.text_document_position_params;
        let (analysis, file) = self.analysis_for(&at.text_document.uri)?;
        let source = analysis.map.get(file);
        let spans = ide::references(&analysis, file, convert::offset(source, at.position), true);
        Some(
            spans
                .into_iter()
                .filter(|(f, _)| *f == file)
                .map(|(_, s)| DocumentHighlight { range: convert::range(source, s), kind: None })
                .collect(),
        )
    }

    fn prepare_rename(
        &mut self,
        p: TextDocumentPositionParams,
    ) -> Result<Option<PrepareRenameResponse>, String> {
        let Some((analysis, file)) = self.analysis_for(&p.text_document.uri) else {
            return Ok(None);
        };
        // Code that does not parse may hide uses of the name, and a rename that misses one
        // breaks the program in a way no diagnostic points back to.
        if !analysis.parsed_cleanly {
            return Err("fix the syntax errors first: a rename could miss uses of the name in code that does not parse".to_owned());
        }
        let source = analysis.map.get(file);
        let span = ide::prepare_rename(&analysis, file, convert::offset(source, p.position))?;
        Ok(Some(PrepareRenameResponse::Range(convert::range(source, span))))
    }

    fn rename(&mut self, p: RenameParams) -> Result<Option<WorkspaceEdit>, String> {
        let at = p.text_document_position;
        let Some((analysis, file)) = self.analysis_for(&at.text_document.uri) else {
            return Ok(None);
        };
        if !analysis.parsed_cleanly {
            return Err("fix the syntax errors first: a rename could miss uses of the name in code that does not parse".to_owned());
        }
        let offset = convert::offset(analysis.map.get(file), at.position);
        let spans = ide::rename(&analysis, file, offset, &p.new_name)?;
        let mut changes: HashMap<Url, Vec<TextEdit>> = HashMap::new();
        for loc in self.locations(&analysis, spans) {
            changes.entry(loc.uri).or_default().push(TextEdit::new(loc.range, p.new_name.clone()));
        }
        Ok(Some(WorkspaceEdit::new(changes)))
    }

    fn symbols(&mut self, p: DocumentSymbolParams) -> Option<DocumentSymbolResponse> {
        let (path, text) = self.text_of(&p.text_document.uri)?;
        let parsed = ide::parse(&path, &text);
        let source = parsed.map.get(parsed.file);
        let decls = ide::decls(source.text(), &parsed.ast);
        Some(DocumentSymbolResponse::Nested(
            decls.iter().map(|d| document_symbol(source, d)).collect(),
        ))
    }

    #[allow(deprecated)] // `SymbolInformation::deprecated` must be spelled out to build one.
    fn workspace_symbols(&mut self, p: WorkspaceSymbolParams) -> Option<WorkspaceSymbolResponse> {
        self.flush();
        let mut out = Vec::new();
        for project in self.projects.values() {
            let Some(analysis) = &project.current else { continue };
            for m in ide::workspace_symbols(analysis, &p.query) {
                let source = analysis.map.get(m.file);
                let Some(uri) = self.uri_for(source.path()) else { continue };
                out.push(SymbolInformation {
                    name: m.name,
                    kind: symbol_kind(m.kind),
                    tags: None,
                    deprecated: None,
                    location: Location::new(uri, convert::range(source, m.span)),
                    container_name: m.container,
                });
            }
        }
        Some(WorkspaceSymbolResponse::Flat(out))
    }

    fn format(&mut self, p: DocumentFormattingParams) -> Option<Vec<TextEdit>> {
        let (path, text) = self.text_of(&p.text_document.uri)?;
        let parsed = ide::parse(&path, &text);
        let formatted = ide::format(&parsed)?;
        if formatted == text {
            return Some(Vec::new());
        }
        let source = parsed.map.get(parsed.file);
        let end = convert::position(source, source.len() as u32);
        Some(vec![TextEdit::new(Range::new(Position::new(0, 0), end), formatted)])
    }

    fn complete(&mut self, p: CompletionParams) -> Option<CompletionResponse> {
        let at = p.text_document_position;
        let (path, text) = self.text_of(&at.text_document.uri)?;
        let key = path_key(&path);
        let mut map = SourceMap::new();
        let id = map.add(path.clone(), text.clone());
        let offset = convert::offset(map.get(id), at.position) as usize;

        // Checked now rather than at the next pause: the receiver of the `.` just typed
        // exists only in this text.
        let current = self.analysis_for(&at.text_document.uri);
        let clean = self.project_of(&key).and_then(|p| p.clean.clone());
        let current = current.as_ref().map(|(a, f)| (a.as_ref(), *f));
        let fallback = clean.as_ref().and_then(|a| Some((a.as_ref(), file_in(a, &key)?)));
        let set = ide::completion(current, fallback, &text, offset);
        Some(CompletionResponse::List(lsp_types::CompletionList {
            is_incomplete: set.incomplete,
            items: set.items.into_iter().map(completion_item).collect(),
        }))
    }

    /// `dazzle/manifestLinks`: the links in a `dazzle.toml` — each recipe's header, and what
    /// each dependency names (spec §17.4). Answered from the analysis of the project the
    /// manifest roots, which is what knows where an include search finds a header.
    fn manifest_links(&mut self, p: ManifestLinksParams) -> Vec<ManifestLink> {
        let Ok(path) = p.uri.to_file_path() else { return Vec::new() };
        let key = path_key(&path);
        if self.projects.get(&key).map_or(true, |p| p.dirty || p.current.is_none()) {
            self.mark_project(&path);
            self.analyse(&key);
        }
        let Some(analysis) = self.projects.get(&key).and_then(|p| p.current.clone()) else {
            return Vec::new();
        };
        ide::manifest_links(&analysis, &path)
            .into_iter()
            .filter_map(|(file, span, target, tooltip)| {
                let source = analysis.map.get(file);
                Some(ManifestLink {
                    range: convert::range(source, span),
                    target: Url::from_file_path(target).ok()?,
                    tooltip,
                })
            })
            .collect()
    }

    fn signature_help(&mut self, p: SignatureHelpParams) -> Option<SignatureHelp> {
        let at = p.text_document_position_params;
        let (analysis, file) = self.analysis_for(&at.text_document.uri)?;
        let source = analysis.map.get(file);
        let sig = ide::signature_help(&analysis, file, convert::offset(source, at.position))?;
        // Parameter offsets are in UTF-16 units, like every other protocol position.
        let units = |b: usize| sig.label[..b].encode_utf16().count() as u32;
        let parameters: Vec<ParameterInformation> = sig
            .params
            .iter()
            .map(|&(s, e)| ParameterInformation {
                label: ParameterLabel::LabelOffsets([units(s), units(e)]),
                documentation: None,
            })
            .collect();
        let active = (!parameters.is_empty()).then(|| sig.active.min(parameters.len() - 1) as u32);
        let documentation = (!sig.docs.is_empty()).then(|| {
            Documentation::MarkupContent(MarkupContent {
                kind: MarkupKind::Markdown,
                value: sig.docs.clone(),
            })
        });
        Some(SignatureHelp {
            signatures: vec![SignatureInformation {
                label: sig.label.clone(),
                documentation,
                parameters: Some(parameters),
                active_parameter: active,
            }],
            active_signature: Some(0),
            active_parameter: active,
        })
    }

    /// Quick fixes for whatever the cursor is on (spec §17.4).
    ///
    /// Every fix is a `quickfix` carrying a plain [`WorkspaceEdit`]: the compiler computed
    /// the replacement, so there is nothing to resolve lazily and no command to run.
    fn code_actions(&mut self, p: CodeActionParams) -> Option<CodeActionResponse> {
        let (analysis, file) = self.analysis_for(&p.text_document.uri)?;
        let source = analysis.map.get(file);
        let (start, end) =
            (convert::offset(source, p.range.start), convert::offset(source, p.range.end));
        let range = Span::new(start.min(end) as usize, start.max(end) as usize);

        let mut out: CodeActionResponse = Vec::new();
        for fix in ide::code_actions(&analysis, file, range) {
            let mut changes: HashMap<Url, Vec<TextEdit>> = HashMap::new();
            // An edit may land in another file than the one asked about, so each is
            // resolved through its own `FileId` rather than assumed to be this buffer.
            let mut ok = true;
            for e in &fix.edits {
                let Some(target) = analysis.map.try_get(e.file) else {
                    ok = false;
                    break;
                };
                let Ok(uri) = Url::from_file_path(target.path()) else {
                    ok = false;
                    break;
                };
                changes.entry(uri).or_default().push(TextEdit {
                    range: convert::range(target, e.span),
                    new_text: e.text.clone(),
                });
            }
            if !ok {
                continue;
            }
            out.push(CodeActionOrCommand::CodeAction(CodeAction {
                title: fix.title,
                kind: Some(CodeActionKind::QUICKFIX),
                edit: Some(WorkspaceEdit { changes: Some(changes), ..Default::default() }),
                ..Default::default()
            }));
        }
        Some(out)
    }

    fn inlay_hints(&mut self, p: InlayHintParams) -> Option<Vec<InlayHint>> {
        let (analysis, file) = self.analysis_for(&p.text_document.uri)?;
        let source = analysis.map.get(file);
        let (start, end) =
            (convert::offset(source, p.range.start), convert::offset(source, p.range.end));
        let range = Span::new(start.min(end) as usize, start.max(end) as usize);
        Some(
            ide::inlay_hints(&analysis, file, range)
                .into_iter()
                .map(|h| {
                    let field = h.kind == ide::HintKind::Field;
                    InlayHint {
                        position: convert::position(source, h.offset),
                        label: InlayHintLabel::String(h.label),
                        kind: Some(if field {
                            InlayHintKind::PARAMETER
                        } else {
                            InlayHintKind::TYPE
                        }),
                        text_edits: None,
                        tooltip: None,
                        padding_left: None,
                        // `id: 1`, not `id:1`: the hint sits before the value it names.
                        padding_right: field.then_some(true),
                        data: None,
                    }
                })
                .collect(),
        )
    }

    fn semantic_tokens(&mut self, p: SemanticTokensParams) -> Option<SemanticTokensResult> {
        let (analysis, file) = self.analysis_for(&p.text_document.uri)?;
        let source = analysis.map.get(file);
        // Each token is placed relative to the one before it, in UTF-16 units.
        let mut data = Vec::new();
        let (mut line, mut col) = (0, 0);
        for t in ide::semantic_tokens(&analysis, file) {
            let start = convert::position(source, t.span.start);
            let end = convert::position(source, t.span.end);
            if start.line != end.line {
                continue;
            }
            let delta_line = start.line - line;
            let delta_start = if delta_line == 0 { start.character - col } else { start.character };
            data.push(SemanticToken {
                delta_line,
                delta_start,
                length: end.character - start.character,
                // The legend is `TokenClass::ALL`, in declaration order.
                token_type: t.class as u32,
                token_modifiers_bitset: u32::from(t.declaration) | (u32::from(t.readonly) << 1),
            });
            (line, col) = (start.line, start.character);
        }
        Some(SemanticTokensResult::Tokens(SemanticTokens { result_id: None, data }))
    }
}

// --- conversions ---------------------------------------------------------------------------

fn token_type(c: ide::TokenClass) -> SemanticTokenType {
    use ide::TokenClass as C;
    match c {
        C::Type => SemanticTokenType::TYPE,
        C::Struct => SemanticTokenType::STRUCT,
        C::Enum => SemanticTokenType::ENUM,
        C::Interface => SemanticTokenType::INTERFACE,
        C::TypeParameter => SemanticTokenType::TYPE_PARAMETER,
        C::Parameter => SemanticTokenType::PARAMETER,
        C::Variable => SemanticTokenType::VARIABLE,
        C::Property => SemanticTokenType::PROPERTY,
        C::EnumMember => SemanticTokenType::ENUM_MEMBER,
        C::Function => SemanticTokenType::FUNCTION,
        C::Method => SemanticTokenType::METHOD,
        C::Macro => SemanticTokenType::MACRO,
        C::Namespace => SemanticTokenType::NAMESPACE,
    }
}

fn key_of(uri: &Url) -> Option<String> {
    uri.to_file_path().ok().map(|p| path_key(&p))
}

/// The nearest `dazzle.toml` in or above the file's directory.
fn find_manifest(path: &Path) -> Option<PathBuf> {
    path.ancestors().skip(1).map(|dir| dir.join("dazzle.toml")).find(|m| m.is_file())
}

fn file_in(analysis: &Analysis, key: &str) -> Option<FileId> {
    if let Some(pseudo) = embedded_for_key(key) {
        return ide::embedded_file(analysis, pseudo);
    }
    analysis.map.iter().find(|f| path_key(f.path()) == key).map(|f| f.id())
}

/// The pseudo-path of the embedded source whose editor copy has path key `key`, if any.
fn embedded_for_key(key: &str) -> Option<&'static str> {
    embedded_copies().iter().find(|(_, copy)| path_key(copy) == key).map(|(pseudo, _)| *pseudo)
}

/// The editor copy of the embedded source at `pseudo` (`<std>/math/vec3.dz`).
fn embedded_copy(pseudo: &str) -> Option<PathBuf> {
    embedded_copies().iter().find(|(p, _)| *p == pseudo).map(|(_, copy)| copy.clone())
}

/// The standard library written out where an editor can open them, so that
/// going to the definition of `String` or `Vec3` lands on something (spec §3.2, §14.4,
/// §17.4).
///
/// They are compiled into `dz`, so they have no files of their own. Each copy is exactly its
/// text — byte offsets and line numbers are the analysis's — under a directory named by a
/// hash of all of them, so two `dz` versions never share one, and marked read-only: editing
/// one would change nothing, and should not look as though it might.
fn embedded_copies() -> &'static [(&'static str, PathBuf)] {
    use std::hash::{Hash, Hasher};
    static COPIES: std::sync::OnceLock<Vec<(&'static str, PathBuf)>> = std::sync::OnceLock::new();
    COPIES.get_or_init(|| {
        let sources = dz_project::embedded_sources();
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        sources.hash(&mut hasher);
        let dir =
            std::env::temp_dir().join("dazzle").join(format!("stdlib-{:016x}", hasher.finish()));
        let mut out = Vec::new();
        for (pseudo, text) in sources {
            // `<std>/math/vec3.dz` → `std/math/vec3.dz`.
            let relative = pseudo.replace(['<', '>'], "");
            let path = dir.join(relative);
            if write_read_only(&path, text).is_some() {
                out.push((pseudo, path));
            }
        }
        out
    })
}

fn write_read_only(path: &Path, text: &str) -> Option<()> {
    if std::fs::read_to_string(path).ok().as_deref() == Some(text) {
        return Some(());
    }
    std::fs::create_dir_all(path.parent()?).ok()?;
    if let Ok(meta) = std::fs::metadata(path) {
        // A partial copy from an interrupted write: make it writable to replace it.
        let mut perms = meta.permissions();
        #[allow(clippy::permissions_set_readonly_false)]
        perms.set_readonly(false);
        let _ = std::fs::set_permissions(path, perms);
    }
    std::fs::write(path, text).ok()?;
    let mut perms = std::fs::metadata(path).ok()?.permissions();
    perms.set_readonly(true);
    let _ = std::fs::set_permissions(path, perms);
    Some(())
}

/// Lex and parse diagnostics for a buffer that no project reads.
fn syntax_diagnostics(doc: &Doc) -> Vec<lsp_types::Diagnostic> {
    let parsed = ide::parse(&doc.path, &doc.text);
    let uri = doc.uri.clone();
    parsed
        .diagnostics
        .iter()
        .filter_map(|d| to_lsp(&parsed.map, d, &|_| Some(uri.clone()), Some(&uri)))
        .map(|(_, d)| d)
        .collect()
}

/// A compiler diagnostic as the protocol's, with the URI it belongs to.
///
/// The primary label becomes the range; every label with something to say becomes related
/// information, which is where an editor shows "value moved here" next to "used here"
/// (spec §6.4). A diagnostic with no span at all lands on the manifest.
fn to_lsp(
    map: &SourceMap,
    d: &dz_diag::Diagnostic,
    uri_for: &dyn Fn(&Path) -> Option<Url>,
    fallback: Option<&Url>,
) -> Option<(Url, lsp_types::Diagnostic)> {
    let (uri, range) = match d.primary_span() {
        Some((file, span)) => {
            let f = map.try_get(file)?;
            (uri_for(f.path())?, convert::range(f, span))
        }
        None => (fallback?.clone(), Range::default()),
    };
    let related: Vec<DiagnosticRelatedInformation> = d
        .labels
        .iter()
        .filter(|l| !l.message.is_empty())
        .filter_map(|l| {
            let f = map.try_get(l.file)?;
            Some(DiagnosticRelatedInformation {
                location: Location::new(uri_for(f.path())?, convert::range(f, l.span)),
                message: l.message.clone(),
            })
        })
        .collect();
    let mut message = d.message.clone();
    for note in &d.notes {
        message.push('\n');
        message.push_str(note);
    }
    let severity = match d.severity {
        dz_diag::Severity::Error => DiagnosticSeverity::ERROR,
        dz_diag::Severity::Warning => DiagnosticSeverity::WARNING,
        dz_diag::Severity::Note => DiagnosticSeverity::INFORMATION,
        dz_diag::Severity::Help => DiagnosticSeverity::HINT,
    };
    Some((
        uri,
        lsp_types::Diagnostic {
            range,
            severity: Some(severity),
            code: d.code.map(|c| NumberOrString::String(c.to_owned())),
            source: Some("dazzle".to_owned()),
            message,
            related_information: if related.is_empty() { None } else { Some(related) },
            // An editor draws "unnecessary" code faded, which is how an unused binding
            // should look: present, but doing nothing.
            tags: (d.code == Some(dz_diag::codes::UNUSED_VARIABLE))
                .then(|| vec![lsp_types::DiagnosticTag::UNNECESSARY]),
            ..Default::default()
        },
    ))
}

fn symbol_kind(kind: ide::DeclKind) -> lsp_types::SymbolKind {
    use ide::DeclKind as K;
    use lsp_types::SymbolKind as S;
    match kind {
        K::Function => S::FUNCTION,
        K::Method => S::METHOD,
        K::Ctor => S::CONSTRUCTOR,
        K::Struct => S::STRUCT,
        K::Union | K::Enum => S::ENUM,
        K::Trait => S::INTERFACE,
        K::Impl => S::OBJECT,
        K::Const => S::CONSTANT,
        K::Macro => S::FUNCTION,
        K::Field => S::FIELD,
        K::Variant => S::ENUM_MEMBER,
        K::TypeAlias => S::TYPE_PARAMETER,
    }
}

#[allow(deprecated)] // `DocumentSymbol::deprecated` must be spelled out to build one.
fn document_symbol(file: &SourceFile, d: &ide::Decl) -> DocumentSymbol {
    let range = convert::range(file, d.span);
    let inside = d.span.start <= d.name_span.start && d.name_span.end <= d.span.end;
    DocumentSymbol {
        // The protocol rejects an empty name, which a half-typed declaration can have.
        name: if d.name.is_empty() { "_".to_owned() } else { d.name.clone() },
        detail: None,
        kind: symbol_kind(d.kind),
        tags: None,
        deprecated: None,
        range,
        selection_range: if inside { convert::range(file, d.name_span) } else { range },
        children: if d.children.is_empty() {
            None
        } else {
            Some(d.children.iter().map(|c| document_symbol(file, c)).collect())
        },
    }
}

fn completion_item(c: ide::Completion) -> CompletionItem {
    use ide::CompletionKind as K;
    CompletionItem {
        kind: Some(match c.kind {
            K::Keyword => CompletionItemKind::KEYWORD,
            K::Type => CompletionItemKind::TYPE_PARAMETER,
            K::Struct => CompletionItemKind::STRUCT,
            K::Enum => CompletionItemKind::ENUM,
            K::Trait => CompletionItemKind::INTERFACE,
            K::Function => CompletionItemKind::FUNCTION,
            K::Method => CompletionItemKind::METHOD,
            K::Field => CompletionItemKind::FIELD,
            K::Variant => CompletionItemKind::ENUM_MEMBER,
            K::Constant => CompletionItemKind::CONSTANT,
            K::Variable => CompletionItemKind::VARIABLE,
            K::Module => CompletionItemKind::MODULE,
            K::Macro => CompletionItemKind::SNIPPET,
        }),
        detail: c.detail,
        // A name reached through a dependency key says which, and sorts after the names in
        // scope bare, which are what a bare name most likely means.
        label_details: c.source.as_ref().map(|s| lsp_types::CompletionItemLabelDetails {
            detail: None,
            description: Some(format!("{s}::")),
        }),
        sort_text: c.source.as_ref().map(|s| format!("~{}~{s}", c.label)),
        filter_text: c.insert.as_ref().map(|_| c.label.clone()),
        insert_text: c.insert,
        label: c.label,
        ..Default::default()
    }
}
