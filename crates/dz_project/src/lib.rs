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

/// The header importer, re-exported so every phase that meets an imported header reads its
/// declarations through the crate that holds them.
pub use dz_cppimport as cppimport;

use crate::toml::{Document, Table, Value};

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

/// A module's C and C++ settings: its `[cpp]` table and the libraries it vendors (spec
/// §15.1). Paths are resolved against the manifest's directory when it is read, and the
/// host target's sections are already folded in.
#[derive(Debug, Clone, Default)]
pub struct CppConfig {
    pub include_dirs: Vec<PathBuf>,
    pub defines: Vec<String>,
    pub lib_dirs: Vec<PathBuf>,
    pub link_libs: Vec<String>,
    pub libs: Vec<CppLib>,
    /// `[cpp.instantiate]`: the C++ type to instantiate, and the Dazzle name to import it
    /// under — `("std::vector<float>", "StdVecF32")` (spec §15.4).
    pub instantiate: Vec<(String, String)>,
}

/// A C or C++ library vendored as source and compiled by `dz`, from a `[cpp.libs.<name>]`
/// recipe (spec §15.1). One with no `sources` is header-only, or ships its binaries.
#[derive(Debug, Clone)]
pub struct CppLib {
    pub name: String,
    /// The recipe's table header, which a build failure points at.
    pub name_span: Span,
    pub sources: Vec<PathBuf>,
    /// `header = "RGFW.h"` — a single-header library. Its declarations are in the header
    /// and its definitions appear only where the implementation macro is defined, so `dz`
    /// compiles one translation unit of its own that includes it with this recipe's
    /// `defines` in force (spec §15.1).
    pub header: Option<PathBuf>,
    pub include_dirs: Vec<PathBuf>,
    pub defines: Vec<String>,
    pub lib_dirs: Vec<PathBuf>,
    pub link_libs: Vec<String>,
}

/// What the preprocessor is given for one module's C++: see [`Workspace::cpp_flags`].
#[derive(Debug, Clone, Default)]
pub struct CppFlags {
    pub include_dirs: Vec<PathBuf>,
    pub defines: Vec<String>,
}

/// The language a vendored source is compiled as, decided by its extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceLang {
    C,
    Cpp,
}

pub fn source_language(path: &Path) -> Option<SourceLang> {
    match path.extension()?.to_str()? {
        "c" => Some(SourceLang::C),
        "cc" | "cpp" | "cxx" => Some(SourceLang::Cpp),
        _ => None,
    }
}

/// The platforms a `[cpp.target.<os>]` section may name (spec §15.1).
pub const TARGETS: &[&str] = &["windows", "linux", "macos", "web"];

/// The target being built for. There is no cross-compilation yet, so it is the host.
pub fn host_target() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "unknown"
    }
}

impl Workspace {
    /// The include dirs and defines one module's C++ is compiled with, and its imported
    /// headers are parsed with: the include dirs of its own `[cpp]` and of every library it
    /// vendors, the defines of its `[cpp]` alone, and the same from everything it depends
    /// on. They travel down the dependency graph because a header means what the defines
    /// say, and every module that includes it has to agree — which is also what lets a
    /// Dazzle module wrapping a library carry the recipe for its users (spec §15.1).
    pub fn cpp_flags(&self, id: ModuleId) -> CppFlags {
        let mut out = CppFlags::default();
        for m in self.reachable(id) {
            let cpp = &self.module(m).cpp;
            let dirs = cpp.include_dirs.iter().chain(cpp.libs.iter().flat_map(|l| &l.include_dirs));
            for dir in dirs {
                if !out.include_dirs.contains(dir) {
                    out.include_dirs.push(dir.clone());
                }
            }
            // A library's own `defines` are not among them: they build *its* sources. A
            // single-header library is the reason — `RGFW_IMPLEMENTATION` compiles the
            // implementation into the one source that asks for it, and defining it for
            // every translation unit that includes the header would define every symbol
            // again, which the linker rejects. A define that has to be shared belongs in
            // `[cpp]`, as §15.1's `JPH_PROFILE_ENABLED` is.
            for d in cpp.defines.iter() {
                if !out.defines.contains(d) {
                    out.defines.push(d.clone());
                }
            }
        }
        out
    }

    /// The template instantiations one module's imports should produce: its own, and those
    /// of everything it depends on, so a Dazzle module wrapping a C++ library can name the
    /// instantiations its users need (spec §15.4, and the same reasoning as `cpp_flags`).
    pub fn cpp_instantiate(&self, id: ModuleId) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = Vec::new();
        for m in self.reachable(id) {
            for pair in &self.module(m).cpp.instantiate {
                if !out.iter().any(|(_, name)| *name == pair.1) {
                    out.push(pair.clone());
                }
            }
        }
        out
    }

    /// Library search directories and libraries to link, from every module the root
    /// reaches, each once.
    pub fn link_inputs(&self) -> (Vec<PathBuf>, Vec<String>) {
        let (mut dirs, mut libs) = (Vec::new(), Vec::new());
        for m in self.reachable(self.root) {
            let cpp = &self.module(m).cpp;
            let module_dirs = cpp.libs.iter().flat_map(|l| &l.lib_dirs).chain(&cpp.lib_dirs);
            for d in module_dirs {
                if !dirs.contains(d) {
                    dirs.push(d.clone());
                }
            }
            for l in cpp.libs.iter().flat_map(|l| &l.link_libs).chain(&cpp.link_libs) {
                if !libs.contains(l) {
                    libs.push(l.clone());
                }
            }
        }
        (dirs, libs)
    }

    /// Every vendored library the root reaches, with the module whose manifest declares it.
    pub fn cpp_libs(&self) -> Vec<(&Module, &CppLib)> {
        self.reachable(self.root)
            .into_iter()
            .flat_map(|m| {
                let module = self.module(m);
                module.cpp.libs.iter().map(move |l| (module, l))
            })
            .collect()
    }

    /// `id` and every module it depends on, directly or not, each once and `id` first.
    fn reachable(&self, id: ModuleId) -> Vec<ModuleId> {
        let mut out = Vec::new();
        let mut stack = vec![id];
        while let Some(m) = stack.pop() {
            if out.contains(&m) {
                continue;
            }
            out.push(m);
            for d in self.module(m).deps.iter().rev() {
                stack.push(d.module);
            }
        }
        out
    }
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
    /// Set on a module that is an imported C or C++ header (spec §15.1). Its declarations
    /// come from libclang rather than from Dazzle source, and nothing is generated for it:
    /// its names are the header's own.
    pub foreign: Option<Foreign>,
    /// The headers this module imports with `import cpp`, as written, each with the module
    /// it became.
    pub cpp_imports: Vec<(String, ModuleId)>,
}

/// What an imported header's module holds.
#[derive(Debug)]
pub struct Foreign {
    pub header: std::sync::Arc<dz_cppimport::Header>,
    /// `header.files` as they were added to the source map, so a declaration's span points
    /// into the header it came from.
    pub files: Vec<FileId>,
}

impl Workspace {
    /// Make an imported header a module of the workspace, and record that `importer`
    /// imports it as `requested`. A header two modules import with the same flags is one
    /// module: the same C declarations, named once.
    pub fn add_foreign(&mut self, importer: ModuleId, requested: &str, foreign: Foreign) -> ModuleId {
        let existing = self
            .modules
            .iter()
            .find(|m| m.foreign.as_ref().is_some_and(|f| std::sync::Arc::ptr_eq(&f.header, &foreign.header)))
            .map(|m| m.id);
        let id = match existing {
            Some(id) => id,
            None => {
                let id = ModuleId(self.modules.len() as u32);
                let header_path = foreign.header.path.clone();
                self.modules.push(Module {
                    id,
                    name: requested.to_owned(),
                    name_span: Span::DUMMY,
                    version: String::new(),
                    root_dir: header_path.parent().map(Path::to_path_buf).unwrap_or_default(),
                    manifest: foreign.files.first().copied().unwrap_or(FileId(0)),
                    manifest_path: header_path,
                    sources: Vec::new(),
                    deps: Vec::new(),
                    build: BuildConfig::default(),
                    cpp: CppConfig::default(),
                    embedded_source: None,
                    foreign: Some(foreign),
                    cpp_imports: Vec::new(),
                });
                id
            }
        };
        self.modules[importer.0 as usize].cpp_imports.push((requested.to_owned(), id));
        id
    }
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

    /// Every module, mutably — for `dz run --release`, which overrides the build settings
    /// the manifests wrote. One command builds one program, so a dependency must not be
    /// left at a different optimisation level than the thing that links it.
    pub fn modules_mut(&mut self) -> impl Iterator<Item = &mut Module> {
        self.modules.iter_mut()
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

    // Two recipes for one library would compile it twice and link its symbols twice.
    let mut seen: HashMap<&str, (FileId, Span)> = HashMap::new();
    for m in &modules {
        for lib in &m.cpp.libs {
            match seen.get(lib.name.as_str()) {
                Some(&(first_file, first_span)) => diags.push(
                    Diagnostic::error(
                        codes::MANIFEST_BAD_VALUE,
                        format!("library `{}` has two recipes", lib.name),
                    )
                    .at(m.manifest, lib.name_span, "declared again here")
                    .with_label(dz_diag::Label::secondary(first_file, first_span, "first declared here"))
                    .with_help("keep one; a module that depends on it sees its headers already (spec §15.1)"),
                ),
                None => {
                    seen.insert(&lib.name, (m.manifest, lib.name_span));
                }
            }
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
            foreign: None,
            cpp_imports: Vec::new(),
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
        let cpp = self.cpp_config(file, &doc, &root_dir);

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
            foreign: None,
            cpp_imports: Vec::new(),
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

    /// `[cpp]`, `[cpp.target.<os>]`, `[cpp.libs.<name>]` and `[cpp.libs.<name>.target.<os>]`
    /// (spec §15.1). A target section adds to its base table and only the host's is kept,
    /// but every section is checked, so a recipe that is wrong for another platform says so
    /// on this one too.
    fn cpp_config(&mut self, file: FileId, doc: &Document, root_dir: &Path) -> CppConfig {
        const MODULE_KEYS: &[&str] = &["include_dirs", "defines", "lib_dirs", "link_libs"];
        const LIB_KEYS: &[&str] =
            &["root", "sources", "header", "include_dirs", "defines", "lib_dirs", "link_libs"];
        const LIB_TARGET_KEYS: &[&str] =
            &["sources", "include_dirs", "defines", "lib_dirs", "link_libs"];
        const MANIFEST_DIR: &str = "the directory holding dazzle.toml";
        const LIB_ROOT: &str = "the library's `root`, or the manifest's directory without one";

        let host = host_target();

        // A library's `root` is needed before any of its paths can be resolved, and nothing
        // stops its target sections coming first, so roots are read in a pass of their own.
        let mut roots: HashMap<String, PathBuf> = HashMap::new();
        for table in &doc.tables {
            let parts: Vec<&str> = table.name.split('.').collect();
            if let ["cpp", "libs", name] = parts[..] {
                let root = match table.get("root") {
                    None => root_dir.to_path_buf(),
                    Some(Value::Str(r, span)) => {
                        let p = normalize(&root_dir.join(r));
                        if !p.is_dir() {
                            self.missing_dir(file, r, *span, MANIFEST_DIR);
                        }
                        p
                    }
                    Some(other) => {
                        self.type_error(file, other, &format!("{}.root", table.name), "a string");
                        root_dir.to_path_buf()
                    }
                };
                roots.insert(name.to_owned(), root);
            }
        }

        let mut module = Lists::default();
        let mut instantiate: Vec<(String, String)> = Vec::new();
        let mut libs: Vec<(String, Span, Lists)> = Vec::new();
        for table in &doc.tables {
            let parts: Vec<&str> = table.name.split('.').collect();
            match parts[..] {
                ["cpp"] => self.cpp_section(file, table, root_dir, MANIFEST_DIR, MODULE_KEYS, &mut module),
                ["cpp", "target", os] => {
                    let applies = self.check_target(file, table, os) && os == host;
                    let mut elsewhere = Lists::default();
                    let out = if applies { &mut module } else { &mut elsewhere };
                    self.cpp_section(file, table, root_dir, MANIFEST_DIR, MODULE_KEYS, out);
                }
                ["cpp", "libs"] if table.entries.is_empty() => {}
                ["cpp", "libs", name] | ["cpp", "libs", name, "target", _] => {
                    let base = roots.get(name).cloned().unwrap_or_else(|| root_dir.to_path_buf());
                    let idx = match libs.iter().position(|(n, ..)| n == name) {
                        Some(i) => i,
                        None => {
                            libs.push((name.to_owned(), table.header_span, Lists::default()));
                            libs.len() - 1
                        }
                    };
                    if let ["cpp", "libs", _, "target", os] = parts[..] {
                        let applies = self.check_target(file, table, os) && os == host;
                        let mut elsewhere = Lists::default();
                        let out = if applies { &mut libs[idx].2 } else { &mut elsewhere };
                        self.cpp_section(file, table, &base, LIB_ROOT, LIB_TARGET_KEYS, out);
                    } else {
                        libs[idx].1 = table.header_span;
                        self.cpp_section(file, table, &base, LIB_ROOT, LIB_KEYS, &mut libs[idx].2);
                    }
                }
                // `"std::vector<float>" = "StdVecF32"`: the instantiations this project
                // wants, each imported as an ordinary concrete type (spec §15.4).
                ["cpp", "instantiate"] => {
                    for entry in &table.entries {
                        match &entry.value {
                            Value::Str(name, _) => {
                                instantiate.push((entry.key.clone(), name.clone()))
                            }
                            other => self.type_error(
                                file,
                                other,
                                &format!("{}.{}", table.name, entry.key),
                                "a string naming the Dazzle type to import it as",
                            ),
                        }
                    }
                }
                ["cpp", ..] => self.diags.push(
                    Diagnostic::error(
                        codes::MANIFEST_BAD_VALUE,
                        format!("unknown table `[{}]`", table.name),
                    )
                    .at(file, table.header_span, "not a table dazzle.toml has")
                    .with_help(
                        "the C++ tables are `[cpp]`, `[cpp.target.<os>]`, `[cpp.libs.<name>]` \
                         and `[cpp.libs.<name>.target.<os>]` (spec §15.1)",
                    ),
                ),
                _ => {}
            }
        }

        CppConfig {
            include_dirs: module.include_dirs,
            defines: module.defines,
            lib_dirs: module.lib_dirs,
            link_libs: module.link_libs,
            instantiate,
            libs: libs
                .into_iter()
                .map(|(name, name_span, l)| CppLib {
                    name,
                    name_span,
                    sources: l.sources,
                    header: l.header,
                    include_dirs: l.include_dirs,
                    defines: l.defines,
                    lib_dirs: l.lib_dirs,
                    link_libs: l.link_libs,
                })
                .collect(),
        }
    }

    /// One C++ table's entries, every one of them a list of strings.
    fn cpp_section(
        &mut self,
        file: FileId,
        table: &Table,
        base: &Path,
        base_desc: &str,
        allowed: &[&str],
        out: &mut Lists,
    ) {
        for entry in &table.entries {
            let key = entry.key.as_str();
            if !allowed.contains(&key) {
                let list = allowed.iter().map(|k| format!("`{k}`")).collect::<Vec<_>>().join(", ");
                self.diags.push(
                    Diagnostic::error(
                        codes::MANIFEST_BAD_VALUE,
                        format!("unknown key `{key}` in `[{}]`", table.name),
                    )
                    .at(file, entry.key_span, "not a key of this table")
                    .with_help(format!("the keys here are {list} (spec §15.1)")),
                );
                continue;
            }
            if key == "root" {
                continue; // read in a pass of its own, before any path is resolved
            }
            // `header = "RGFW.h"`: a single-header library, whose implementation `dz`
            // compiles itself rather than making you write a one-line `.c` for it
            // (spec §15.1). A string, not a list — there is one of them.
            if key == "header" {
                match &entry.value {
                    Value::Str(s, span) => {
                        let path = normalize(&base.join(s));
                        if !path.is_file() {
                            self.diags.push(
                                Diagnostic::error(
                                    codes::MANIFEST_BAD_VALUE,
                                    format!("`{s}` is not a file"),
                                )
                                .at(file, *span, "no header here")
                                .with_help(format!("the path is relative to {base_desc}")),
                            );
                        }
                        out.header = Some(path);
                    }
                    other => self.type_error(
                        file,
                        other,
                        &format!("{}.{key}", table.name),
                        "a string naming the library's header",
                    ),
                }
                continue;
            }
            let Value::Array(items, _) = &entry.value else {
                self.type_error(file, &entry.value, &format!("{}.{key}", table.name), "an array of strings");
                continue;
            };
            for item in items {
                let Value::Str(s, span) = item else {
                    self.diags.push(
                        Diagnostic::error(
                            codes::MANIFEST_BAD_VALUE,
                            format!("`{}.{key}` entries must be strings", table.name),
                        )
                        .at(file, item.span(), format!("this is {}", item.type_name())),
                    );
                    continue;
                };
                match key {
                    "defines" => out.defines.push(s.clone()),
                    "link_libs" => out.link_libs.push(s.clone()),
                    "sources" => self.lib_sources(file, base, base_desc, s, *span, &mut out.sources),
                    _ => {
                        let dir = normalize(&base.join(s));
                        if !dir.is_dir() {
                            self.missing_dir(file, s, *span, base_desc);
                        }
                        let list = if key == "include_dirs" { &mut out.include_dirs } else { &mut out.lib_dirs };
                        if !list.contains(&dir) {
                            list.push(dir);
                        }
                    }
                }
            }
        }
    }

    /// One `sources` entry of a library: a path or a glob. A glob may sweep up headers and
    /// other files, which are skipped; a path written out must be a C or C++ source and
    /// must exist.
    fn lib_sources(
        &mut self,
        file: FileId,
        base: &Path,
        base_desc: &str,
        pattern: &str,
        span: Span,
        out: &mut Vec<PathBuf>,
    ) {
        let is_glob = pattern.contains(['*', '?', '[']);
        let joined = normalize(&base.join(pattern)).to_string_lossy().replace('\\', "/");
        let matches: Vec<PathBuf> = match glob::glob(&joined) {
            Ok(m) => m.flatten().filter(|p| p.is_file()).collect(),
            Err(e) => {
                self.diags.push(
                    Diagnostic::error(codes::MANIFEST_BAD_VALUE, format!("invalid source pattern: {e}"))
                        .at(file, span, "not a valid glob"),
                );
                return;
            }
        };
        let mut found = 0usize;
        for p in &matches {
            if source_language(p).is_none() {
                if !is_glob {
                    self.diags.push(
                        Diagnostic::error(
                            codes::MANIFEST_BAD_VALUE,
                            format!("`{pattern}` is not a C or C++ source"),
                        )
                        .at(file, span, "not compilable")
                        .with_help(
                            "a library source ends in .c, .cc, .cpp or .cxx; headers are found \
                             through `include_dirs` (spec §15.1)",
                        ),
                    );
                    return;
                }
                continue;
            }
            found += 1;
            if !out.contains(p) {
                out.push(p.clone());
            }
        }
        if found == 0 {
            let (msg, label) = if is_glob {
                (format!("`{pattern}` matched no C or C++ sources"), "nothing here")
            } else {
                (format!("source `{pattern}` does not exist"), "no such file")
            };
            self.diags.push(
                Diagnostic::error(codes::MANIFEST_NO_SOURCES, msg)
                    .at(file, span, label)
                    .with_help(format!("the path is relative to {base_desc}")),
            );
        }
    }

    fn check_target(&mut self, file: FileId, table: &Table, os: &str) -> bool {
        if TARGETS.contains(&os) {
            return true;
        }
        self.diags.push(
            Diagnostic::error(codes::MANIFEST_BAD_VALUE, format!("unknown target `{os}`"))
                .at(file, table.header_span, "not a target dz builds for")
                .with_help(format!("targets are {} (spec §15.1)", TARGETS.join(", "))),
        );
        false
    }

    fn missing_dir(&mut self, file: FileId, path: &str, span: Span, base_desc: &str) {
        self.diags.push(
            Diagnostic::error(codes::MANIFEST_BAD_VALUE, format!("directory `{path}` does not exist"))
                .at(file, span, "no such directory")
                .with_help(format!("the path is relative to {base_desc}")),
        );
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

/// The lists one C++ table contributes, gathered before they become a [`CppConfig`].
#[derive(Default)]
struct Lists {
    sources: Vec<PathBuf>,
    header: Option<PathBuf>,
    include_dirs: Vec<PathBuf>,
    defines: Vec<String>,
    lib_dirs: Vec<PathBuf>,
    link_libs: Vec<String>,
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
