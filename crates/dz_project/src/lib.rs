//! Projects and the workspace graph (spec §3.1).
//!
//! **A module is a project and a project is a module.** A `dazzle.toml` declares one
//! module and lists the files that belong to it; folders carry no meaning. Dependencies
//! name other manifests by path, and the graph they form must be acyclic (spec §3.2).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use dz_diag::{codes, Diagnostic, Diagnostics};
use dz_span::{FileId, SourceMap, Span};

pub mod toml;

use crate::toml::{Document, Value};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct ModuleId(pub u32);

#[derive(Debug, Clone)]
pub struct BuildConfig {
    pub cpp_standard: String,
    pub output: OutputKind,
    pub optimize: Optimize,
    pub type_info: TypeInfo,
}

impl Default for BuildConfig {
    fn default() -> Self {
        BuildConfig {
            cpp_standard: "c++17".to_owned(),
            output: OutputKind::Executable,
            optimize: Optimize::Debug,
            type_info: TypeInfo::Used,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputKind {
    Executable,
    StaticLib,
    SharedLib,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Optimize {
    Debug,
    Release,
    Shipping,
}

/// How much of the type table to emit (spec §13.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeInfo {
    None,
    Used,
    All,
}

#[derive(Debug, Clone, Default)]
pub struct CppConfig {
    pub include_dirs: Vec<String>,
    pub link_libs: Vec<String>,
    pub defines: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Dependency {
    /// The key in `[dependencies]`, which is how this module refers to it in `import`.
    pub alias: String,
    pub alias_span: Span,
    pub module: ModuleId,
    /// True for the prelude, which every module depends on without saying so (spec §3.2).
    /// An unused-import warning would be nonsense for a dependency nobody wrote.
    pub implicit: bool,
}

#[derive(Debug)]
pub struct Module {
    pub id: ModuleId,
    /// Declared name, which may contain `::` to place the module in a namespace.
    pub name: String,
    pub name_span: Span,
    pub version: String,
    pub root_dir: PathBuf,
    pub manifest: FileId,
    pub manifest_path: PathBuf,
    /// Source files, in a deterministic order.
    pub sources: Vec<PathBuf>,
    pub deps: Vec<Dependency>,
    pub build: BuildConfig,
    pub cpp: CppConfig,
    /// Set only on the **prelude** (spec §3.2, §18.23): its source is carried by the
    /// compiler rather than read from disk, and `sources` holds one pseudo-path for it so
    /// ordering, spans and diagnostics all work exactly as they do for a real file.
    pub embedded_source: Option<&'static str>,
}

impl Module {
    /// Look up a dependency by the alias used in `import`.
    pub fn dependency(&self, alias: &str) -> Option<&Dependency> {
        self.deps.iter().find(|d| d.alias == alias)
    }

    /// The bytes of one of this module's sources. Every phase goes through here rather
    /// than calling `std::fs::read`, so the prelude needs no special case downstream.
    pub fn read_source(&self, path: &Path) -> std::io::Result<Vec<u8>> {
        match self.embedded_source {
            Some(text) => Ok(text.as_bytes().to_vec()),
            None => std::fs::read(path),
        }
    }

    /// Is this the compiler's own prelude (spec §18.23)? It is emitted like any other
    /// module — `Ordering` and `Formatter` are real types user code can name — so this is
    /// for diagnostics and for the "did you mean" lists, not for skipping work.
    pub fn is_prelude(&self) -> bool {
        self.embedded_source.is_some()
    }
}

#[derive(Debug)]
pub struct Workspace {
    modules: Vec<Module>,
    pub root: ModuleId,
    /// The prelude (spec §3.2, §18.23). Always present, and always module 0.
    pub prelude: ModuleId,
}

impl Workspace {
    pub fn module(&self, id: ModuleId) -> &Module {
        &self.modules[id.0 as usize]
    }

    pub fn modules(&self) -> impl Iterator<Item = &Module> {
        self.modules.iter()
    }

    pub fn len(&self) -> usize {
        self.modules.len()
    }

    pub fn is_empty(&self) -> bool {
        self.modules.is_empty()
    }

    /// Find a module by its declared name.
    pub fn by_name(&self, name: &str) -> Option<&Module> {
        self.modules.iter().find(|m| m.name == name)
    }
}

/// Load a workspace rooted at a `dazzle.toml`, or at a directory containing one.
///
/// Returns `None` only when the root manifest could not be read at all; every other
/// problem is reported as a diagnostic and loading continues, so one broken dependency
/// does not hide the rest.
pub fn load(
    entry: &Path,
    map: &mut SourceMap,
    diags: &mut Diagnostics,
) -> Option<Workspace> {
    let manifest_path =
        normalize(&if entry.is_dir() { entry.join("dazzle.toml") } else { entry.to_owned() });

    let mut loader = Loader { map, diags, modules: Vec::new(), by_path: HashMap::new() };
    let prelude = loader.add_prelude();
    let root = loader.load_module(&manifest_path, None)?;

    // Every module depends on the prelude, so `Ord` and `Iterate` are in scope everywhere
    // with no import (spec §3.2, §18.23). Recorded as an ordinary dependency rather than as
    // a special case in name resolution, so visibility, "did you mean", and the cycle check
    // all treat it like any other module.
    let mut modules = loader.modules;
    for m in &mut modules {
        if m.id != prelude {
            m.deps.push(Dependency {
                alias: PRELUDE_MODULE.to_owned(),
                alias_span: Span::DUMMY,
                module: prelude,
                implicit: true,
            });
        }
    }
    Some(Workspace { modules, root, prelude })
}

/// The prelude's module name (spec §3.2). `dz::Ord` always reaches it, and a local
/// declaration of the same name shadows the unqualified spelling.
pub const PRELUDE_MODULE: &str = "dz";

/// The prelude's source, compiled into the binary.
const PRELUDE_SOURCE: &str = include_str!("prelude.dz");

struct Loader<'a> {
    map: &'a mut SourceMap,
    diags: &'a mut Diagnostics,
    modules: Vec<Module>,
    /// Canonical manifest path to module, so a diamond dependency loads once.
    by_path: HashMap<PathBuf, ModuleId>,
}

impl<'a> Loader<'a> {
    /// Add the prelude as module 0 (spec §3.2, §18.23).
    ///
    /// It is a real module in every respect except that its source comes from the compiler
    /// rather than from disk: it goes into the `SourceMap` under a pseudo-path, so a
    /// diagnostic inside it renders with a caret and a line number exactly as any other
    /// file's would. That matters — a mistake in the prelude is a compiler bug, and it
    /// should be as readable as a user's.
    fn add_prelude(&mut self) -> ModuleId {
        let id = ModuleId(self.modules.len() as u32);
        let path = PathBuf::from("<prelude>").join("dz.dz");
        let file = self.map.add(path.clone(), PRELUDE_SOURCE.to_owned());
        self.modules.push(Module {
            id,
            name: PRELUDE_MODULE.to_owned(),
            name_span: Span::DUMMY,
            version: env!("CARGO_PKG_VERSION").to_owned(),
            root_dir: PathBuf::from("<prelude>"),
            manifest: file,
            manifest_path: path.clone(),
            sources: vec![path],
            deps: Vec::new(),
            build: BuildConfig::default(),
            cpp: CppConfig::default(),
            embedded_source: Some(PRELUDE_SOURCE),
        });
        id
    }

    /// `from` is the manifest that depends on this one, used for the cycle message.
    fn load_module(&mut self, manifest_path: &Path, from: Option<(FileId, Span)>) -> Option<ModuleId> {
        let canonical =
            manifest_path.canonicalize().unwrap_or_else(|_| manifest_path.to_path_buf());
        if let Some(id) = self.by_path.get(&canonical) {
            return Some(*id);
        }

        let bytes = match std::fs::read(manifest_path) {
            Ok(b) => b,
            Err(e) => {
                let msg = format!("cannot read {}: {e}", manifest_path.display());
                match from {
                    Some((file, span)) => self.diags.push(
                        Diagnostic::error(codes::MANIFEST_MISSING, msg)
                            .at(file, span, "this dependency's manifest does not exist")
                            .with_help("the path is relative to the manifest that declares it"),
                    ),
                    None => self
                        .diags
                        .push(Diagnostic::error(codes::MANIFEST_MISSING, msg)),
                }
                return None;
            }
        };

        let file = match self.map.add_bytes(manifest_path, &bytes) {
            Ok(f) => f,
            Err(e) => {
                self.diags.push(Diagnostic::error(
                    codes::MANIFEST_SYNTAX,
                    format!("{}: {e}", manifest_path.display()),
                ));
                return None;
            }
        };

        let src = self.map.get(file).text().to_owned();
        let doc = toml::parse(file, &src, self.diags);

        let root_dir = manifest_path.parent().unwrap_or(Path::new(".")).to_path_buf();
        let id = ModuleId(self.modules.len() as u32);

        // Reserve the slot before loading dependencies so a cycle is detected rather than
        // recursing forever.
        self.by_path.insert(canonical, id);

        let (name, name_span) = self.module_name(file, &doc);
        let version = match doc.table("module").and_then(|t| t.get("version")) {
            Some(v) => match as_str(v) {
                Some(s) => s.to_owned(),
                None => {
                    self.type_error(file, v, "module.version", "a string");
                    "0.0.0".to_owned()
                }
            },
            None => "0.0.0".to_owned(),
        };
        let sources = self.expand_sources(file, &doc, &root_dir, &name);
        let build = self.build_config(file, &doc);
        let cpp = self.cpp_config(file, &doc);

        self.modules.push(Module {
            id,
            name,
            name_span,
            version,
            root_dir: root_dir.clone(),
            manifest: file,
            manifest_path: manifest_path.to_path_buf(),
            sources,
            deps: Vec::new(),
            build,
            cpp,
            embedded_source: None,
        });

        let deps = self.load_dependencies(file, &doc, &root_dir, id);
        self.modules[id.0 as usize].deps = deps;

        Some(id)
    }

    fn module_name(&mut self, file: FileId, doc: &Document) -> (String, Span) {
        let Some(table) = doc.table("module") else {
            self.diags.push(
                Diagnostic::error(codes::MANIFEST_MISSING_FIELD, "manifest has no `[module]` table")
                    .at(file, Span::at(0), "expected a `[module]` table here")
                    .with_help("every dazzle.toml needs `[module]` with a `name` (spec §3.1)"),
            );
            return (String::new(), Span::DUMMY);
        };
        let Some(entry) = table.entry("name") else {
            self.diags.push(
                Diagnostic::error(codes::MANIFEST_MISSING_FIELD, "`[module]` has no `name`")
                    .at(file, table.header_span, "this table needs a `name` entry")
                    .with_help("for example: name = \"engine\""),
            );
            return (String::new(), table.header_span);
        };
        let Value::Str(name, span) = &entry.value else {
            self.type_error(file, &entry.value, "module.name", "a string");
            return (String::new(), entry.value.span());
        };

        // A module name is `ident` or `ident::ident::…` (spec §3.2).
        let valid = !name.is_empty()
            && name.split("::").all(|seg| {
                !seg.is_empty()
                    && seg.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_')
                    && seg.chars().all(|c| c.is_alphanumeric() || c == '_')
            });
        if !valid {
            self.diags.push(
                Diagnostic::error(
                    codes::MANIFEST_BAD_VALUE,
                    format!("`{name}` is not a valid module name"),
                )
                .at(file, *span, "invalid module name")
                .with_help(
                    "a name is an identifier, or several joined by `::` — for example \
                     \"engine\" or \"engine::render\" (spec §3.2)",
                ),
            );
        }
        (name.clone(), *span)
    }

    fn expand_sources(
        &mut self,
        file: FileId,
        doc: &Document,
        root_dir: &Path,
        module_name: &str,
    ) -> Vec<PathBuf> {
        let Some(table) = doc.table("module") else { return Vec::new() };
        let Some(entry) = table.entry("sources") else {
            self.diags.push(
                Diagnostic::error(codes::MANIFEST_MISSING_FIELD, "`[module]` has no `sources`")
                    .at(file, table.header_span, "this table needs a `sources` entry")
                    .with_help("for example: sources = [\"src/**/*.dz\"]"),
            );
            return Vec::new();
        };
        let Value::Array(patterns, _) = &entry.value else {
            self.type_error(file, &entry.value, "module.sources", "an array of strings");
            return Vec::new();
        };

        let excludes: Vec<String> = match table.get("exclude") {
            Some(Value::Array(items, _)) => {
                items.iter().filter_map(|v| as_str(v).map(str::to_owned)).collect()
            }
            Some(other) => {
                self.type_error(file, other, "module.exclude", "an array of strings");
                Vec::new()
            }
            None => Vec::new(),
        };
        let exclude_matchers: Vec<glob::Pattern> = excludes
            .iter()
            .filter_map(|p| glob::Pattern::new(&root_dir.join(p).to_string_lossy()).ok())
            .collect();

        let mut out: Vec<PathBuf> = Vec::new();
        for pattern in patterns {
            let Value::Str(pat, span) = pattern else {
                self.type_error(file, pattern, "module.sources entry", "a string");
                continue;
            };
            let joined = root_dir.join(pat);
            let joined = joined.to_string_lossy().replace('\\', "/");
            let matches = match glob::glob(&joined) {
                Ok(m) => m,
                Err(e) => {
                    self.diags.push(
                        Diagnostic::error(
                            codes::MANIFEST_BAD_VALUE,
                            format!("invalid source pattern: {e}"),
                        )
                        .at(file, *span, "not a valid glob"),
                    );
                    continue;
                }
            };
            let mut found = 0usize;
            for entry in matches.flatten() {
                if entry.is_dir() {
                    continue;
                }
                if exclude_matchers.iter().any(|m| m.matches_path(&entry)) {
                    continue;
                }
                found += 1;
                if !out.contains(&entry) {
                    out.push(entry);
                }
            }
            if found == 0 {
                self.diags.push(
                    Diagnostic::warning(
                        codes::MANIFEST_NO_SOURCES,
                        format!("source pattern `{pat}` matched no files"),
                    )
                    .at(file, *span, "nothing here"),
                );
            }
        }

        if out.is_empty() {
            self.diags.push(
                Diagnostic::error(
                    codes::MANIFEST_NO_SOURCES,
                    format!("module `{module_name}` has no source files"),
                )
                .at(file, entry.value.span(), "these patterns matched nothing")
                .with_help("paths are relative to the directory holding dazzle.toml"),
            );
        }
        // Deterministic order, so diagnostics and codegen do not depend on the filesystem.
        out.sort();
        out
    }

    fn load_dependencies(
        &mut self,
        file: FileId,
        doc: &Document,
        root_dir: &Path,
        self_id: ModuleId,
    ) -> Vec<Dependency> {
        let Some(table) = doc.table("dependencies") else { return Vec::new() };
        let mut deps: Vec<Dependency> = Vec::new();

        for entry in &table.entries {
            let Value::InlineTable(fields, span) = &entry.value else {
                self.type_error(
                    file,
                    &entry.value,
                    &format!("dependencies.{}", entry.key),
                    "a table like { path = \"../libs/math\" }",
                );
                continue;
            };
            let path_value = fields.iter().find(|f| f.key == "path");
            let Some(path_entry) = path_value else {
                self.diags.push(
                    Diagnostic::error(
                        codes::MANIFEST_MISSING_FIELD,
                        format!("dependency `{}` has no `path`", entry.key),
                    )
                    .at(file, *span, "expected `path = \"...\"`")
                    .with_help("only path dependencies are supported so far"),
                );
                continue;
            };
            let Value::Str(rel, path_span) = &path_entry.value else {
                self.type_error(
                    file,
                    &path_entry.value,
                    &format!("dependencies.{}.path", entry.key),
                    "a string",
                );
                continue;
            };

            if let Some(prev) = deps.iter().find(|d| d.alias == entry.key) {
                let prev_span = prev.alias_span;
                self.diags.push(
                    Diagnostic::error(
                        codes::MANIFEST_BAD_VALUE,
                        format!("duplicate dependency `{}`", entry.key),
                    )
                    .at(file, entry.key_span, "declared twice")
                    .with_label(dz_diag::Label::secondary(file, prev_span, "first declared here")),
                );
                continue;
            }

            // A dependency path names a directory unless it points straight at a
            // manifest. Deciding by extension rather than by `is_dir` keeps the error
            // message sensible when the directory does not exist at all.
            let dep_path = normalize(&root_dir.join(rel));
            let dep_manifest = if dep_path.extension().is_some_and(|e| e == "toml") {
                dep_path
            } else {
                dep_path.join("dazzle.toml")
            };

            match self.load_module(&dep_manifest, Some((file, *path_span))) {
                Some(dep_id) => {
                    if dep_id == self_id {
                        self.diags.push(
                            Diagnostic::error(
                                codes::MODULE_CYCLE,
                                "a module cannot depend on itself",
                            )
                            .at(file, *path_span, "this points back at this module"),
                        );
                        continue;
                    }
                    if self.depends_on(dep_id, self_id) {
                        let a = self.modules[self_id.0 as usize].name.clone();
                        let b = self.modules[dep_id.0 as usize].name.clone();
                        self.diags.push(
                            Diagnostic::error(
                                codes::MODULE_CYCLE,
                                format!("circular dependency between `{a}` and `{b}`"),
                            )
                            .at(file, *path_span, "this closes a cycle")
                            .with_help(
                                "modules may not depend on each other in a cycle (spec §3.2); \
                                 move the shared code into a third module",
                            ),
                        );
                        continue;
                    }
                    deps.push(Dependency {
                        alias: entry.key.clone(),
                        alias_span: entry.key_span,
                        module: dep_id,
                        implicit: false,
                    });
                }
                None => continue,
            }
        }
        deps
    }

    /// Is `target` reachable from `start` through the dependency edges loaded so far?
    fn depends_on(&self, start: ModuleId, target: ModuleId) -> bool {
        let mut stack = vec![start];
        let mut seen = vec![false; self.modules.len()];
        while let Some(m) = stack.pop() {
            if m == target {
                return true;
            }
            if seen[m.0 as usize] {
                continue;
            }
            seen[m.0 as usize] = true;
            for d in &self.modules[m.0 as usize].deps {
                stack.push(d.module);
            }
        }
        false
    }

    fn build_config(&mut self, file: FileId, doc: &Document) -> BuildConfig {
        let mut cfg = BuildConfig::default();
        let Some(table) = doc.table("build") else { return cfg };

        if let Some(v) = table.get("cpp_standard") {
            match as_str(v) {
                Some(s) => cfg.cpp_standard = s.to_owned(),
                None => self.type_error(file, v, "build.cpp_standard", "a string"),
            }
        }
        if let Some(v) = table.get("output") {
            cfg.output = match as_str(v) {
                Some("executable") => OutputKind::Executable,
                Some("static-lib") => OutputKind::StaticLib,
                Some("shared-lib") => OutputKind::SharedLib,
                _ => {
                    self.enum_error(
                        file,
                        v,
                        "build.output",
                        &["executable", "static-lib", "shared-lib"],
                    );
                    cfg.output
                }
            };
        }
        if let Some(v) = table.get("optimize") {
            cfg.optimize = match as_str(v) {
                Some("debug") => Optimize::Debug,
                Some("release") => Optimize::Release,
                Some("shipping") => Optimize::Shipping,
                _ => {
                    self.enum_error(file, v, "build.optimize", &["debug", "release", "shipping"]);
                    cfg.optimize
                }
            };
        }
        if let Some(v) = table.get("type_info") {
            cfg.type_info = match as_str(v) {
                Some("none") => TypeInfo::None,
                Some("used") => TypeInfo::Used,
                Some("all") => TypeInfo::All,
                _ => {
                    self.enum_error(file, v, "build.type_info", &["none", "used", "all"]);
                    cfg.type_info
                }
            };
        }
        cfg
    }

    fn cpp_config(&mut self, file: FileId, doc: &Document) -> CppConfig {
        let mut cfg = CppConfig::default();
        let Some(table) = doc.table("cpp") else { return cfg };
        for (key, out) in [
            ("include_dirs", &mut cfg.include_dirs),
            ("link_libs", &mut cfg.link_libs),
            ("defines", &mut cfg.defines),
        ] {
            match table.get(key) {
                Some(Value::Array(items, _)) => {
                    for item in items {
                        match as_str(item) {
                            Some(s) => out.push(s.to_owned()),
                            None => {
                                self.diags.push(
                                    Diagnostic::error(
                                        codes::MANIFEST_BAD_VALUE,
                                        format!("`cpp.{key}` entries must be strings"),
                                    )
                                    .at(file, item.span(), format!("this is {}", item.type_name())),
                                );
                            }
                        }
                    }
                }
                Some(other) => self.type_error(file, other, &format!("cpp.{key}"), "an array of strings"),
                None => {}
            }
        }
        cfg
    }

    fn type_error(&mut self, file: FileId, value: &Value, what: &str, expected: &str) {
        self.diags.push(
            Diagnostic::error(
                codes::MANIFEST_BAD_VALUE,
                format!("`{what}` must be {expected}"),
            )
            .at(file, value.span(), format!("this is {}", value.type_name())),
        );
    }

    fn enum_error(&mut self, file: FileId, value: &Value, what: &str, allowed: &[&str]) {
        let list = allowed.iter().map(|a| format!("\"{a}\"")).collect::<Vec<_>>().join(", ");
        self.diags.push(
            Diagnostic::error(codes::MANIFEST_BAD_VALUE, format!("invalid value for `{what}`"))
                .at(file, value.span(), "not one of the allowed values")
                .with_help(format!("valid values are {list}")),
        );
    }
}

fn as_str(v: &Value) -> Option<&str> {
    match v {
        Value::Str(s, _) => Some(s),
        _ => None,
    }
}

/// Collapse `.` and `..` lexically.
///
/// Not `canonicalize`: that requires the path to exist and, on Windows, returns a `\\?\`
/// UNC path that would then show up in every diagnostic. Paths here only ever appear in
/// messages, so a purely textual normalisation is both sufficient and prettier.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                // Only pop a real directory name; keep `..` that would escape the root.
                let popped = matches!(
                    out.components().next_back(),
                    Some(std::path::Component::Normal(_))
                ) && out.pop();
                if !popped {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    if out.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        out
    }
}
