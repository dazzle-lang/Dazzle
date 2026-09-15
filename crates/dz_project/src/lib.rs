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
    /// `cxx = "g++"` — the C++ compiler that builds this project's generated code, and
    /// the vendored libraries alongside it (spec §16.5).
    ///
    /// `None` lets `dz` pick the first of `clang++`, `g++`, `c++` it finds. A
    /// `[build.target.<os>]` section overrides it per platform, and `--cxx` on the command
    /// line overrides both, so "which compiler" is answerable at whichever level actually
    /// knows: the project, the platform, or the person running the build.
    pub cxx: Option<String>,
    pub output: OutputKind,
    pub optimize: Optimize,
    pub type_info: TypeInfo,
    /// `track_memory = true`: turn on the prelude's `TRACK_MEMORY` flag, which the
    /// standard allocators read (spec §12.10, §14.2). Only the root manifest's counts.
    pub track_memory: bool,
}

/// The flags the compiler defines in every package, set from the build mode and the host
/// (spec §12.10). A package may not declare one of these itself.
pub const BUILTIN_FLAGS: &[&str] =
    &["DEBUG", "RELEASE", "SHIPPING", "OS_WINDOWS", "OS_LINUX", "OS_MACOS"];

/// `NAME = false` or `NAME = { default = false, doc = "…" }` under `[flags]` (spec §12.10).
#[derive(Debug, Clone)]
pub struct FlagDecl {
    pub name: String,
    pub span: Span,
    pub default: bool,
    pub doc: Option<String>,
}

impl Default for BuildConfig {
    fn default() -> Self {
        BuildConfig {
            cpp_standard: "c++17".to_owned(),
            cxx: None,
            output: OutputKind::Executable,
            optimize: Optimize::Debug,
            type_info: TypeInfo::Used,
            track_memory: false,
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
    /// `frameworks = ["Cocoa", "IOKit"]` — macOS only, where a system library is a
    /// framework and `-lCocoa` finds nothing. Linked as `-framework <name>`; ignored on
    /// every other platform, so a recipe names them in a `[….target.macos]` section
    /// (spec §15.1).
    pub frameworks: Vec<String>,
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
    /// See [`CppConfig::frameworks`].
    pub frameworks: Vec<String>,
    /// `flags = ["-x", "objective-c"]` — passed to the compiler for **this library's**
    /// sources, after the ones `dz` chooses, so a recipe can say something `defines` and
    /// `include_dirs` cannot (spec §15.1).
    ///
    /// The case that forced it: raylib's `rglfw.c` includes GLFW's Cocoa backend, which is
    /// Objective-C, so on macOS that one translation unit has to be compiled as Objective-C
    /// even though it is named `.c`. raylib's own Makefile does the same thing.
    ///
    /// Only a vendored library's, never the generated code's: invariant 7 says Dazzle's own
    /// output must compile warning-clean under a fixed set of flags, and that is not
    /// something a manifest gets to weaken.
    pub flags: Vec<String>,
    /// `import = ["raylib.h"]`: the header Dazzle code sees of this library, as a
    /// `cpp = "…"` dependency names it (spec §15.1).
    pub import: Vec<(String, Span)>,
}

/// Everything the final link needs, gathered from the whole dependency graph.
#[derive(Debug, Clone, Default)]
pub struct LinkInputs {
    pub dirs: Vec<PathBuf>,
    pub libs: Vec<String>,
    /// macOS frameworks, linked as `-framework <name>`. Empty everywhere else.
    pub frameworks: Vec<String>,
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

    /// Library search directories, libraries, and macOS frameworks to link — from every
    /// module the root reaches, each once.
    pub fn link_inputs(&self) -> LinkInputs {
        let (mut dirs, mut libs, mut frameworks) = (Vec::new(), Vec::new(), Vec::new());
        for m in self.reachable(self.root) {
            let cpp = &self.module(m).cpp;
            for f in cpp.libs.iter().flat_map(|l| &l.frameworks).chain(&cpp.frameworks) {
                if !frameworks.contains(f) {
                    frameworks.push(f.clone());
                }
            }
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
        LinkInputs { dirs, libs, frameworks }
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
    /// The key in `[dependencies]`: the name every file of this module reaches it by
    /// (spec §3.2).
    pub alias: String,
    pub alias_span: Span,
    pub module: ModuleId,
    /// True for the standard library when the manifest does not list it. The module still
    /// depends on it — the language's own constructs are written in it — but cannot name
    /// anything in it (spec §3.2, §14.4).
    pub implicit: bool,
    /// `flags = ["PROFILE"]`: the dependency's flags this module turns on (spec §12.10).
    pub flags: Vec<(String, Span)>,
    /// `open = true`: its exports are also written bare (spec §3.2).
    pub open: bool,
    /// `pub = true`: modules depending on this one reach it as `this::alias` (spec §3.2).
    pub public: bool,
    /// The value that says where it comes from — `"../libs/math"`, `"raylib"` — which an
    /// editor makes a link (spec §17.4). The key's span for `std`, a dummy for an implicit one.
    pub source_span: Span,
}

/// `rl = { cpp = "raylib" }`: the header of this module's `[cpp.libs.raylib]` recipe, as a
/// dependency (spec §15.1). It becomes a [`Dependency`] once the driver has read the header,
/// which needs every module's include dirs, so the loader only records it.
#[derive(Debug, Clone)]
pub struct CppDep {
    pub alias: String,
    pub alias_span: Span,
    pub recipe: String,
    pub recipe_span: Span,
    pub open: bool,
    pub public: bool,
}

/// `shapes = { via = "rl::shapes" }`: a public dependency of a dependency, named here
/// (spec §3.2). Resolved by name resolution, which walks the path through the keys.
#[derive(Debug, Clone)]
pub struct ViaDep {
    pub alias: String,
    pub alias_span: Span,
    /// The path's segments: `["rl", "shapes"]`.
    pub path: Vec<String>,
    pub path_span: Span,
    pub open: bool,
    pub public: bool,
}

/// `Vec2 = "std"` under `[names]`: which opened dependency a bare name means (spec §3.2).
#[derive(Debug, Clone)]
pub struct NamePick {
    pub name: String,
    pub name_span: Span,
    /// A dependency key of this module.
    pub dep: String,
    pub dep_span: Span,
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
    /// `cpp = "…"` dependencies not yet read; see [`CppDep`].
    pub cpp_deps: Vec<CppDep>,
    pub via_deps: Vec<ViaDep>,
    /// `[names]` (spec §3.2).
    pub names: Vec<NamePick>,
    /// The flags this package declares under `[flags]` (spec §12.10).
    pub flags: Vec<FlagDecl>,
    pub build: BuildConfig,
    pub cpp: CppConfig,
    /// Set only on the **standard library** (spec §14.4): its sources are carried by the
    /// compiler rather than read from disk, under pseudo-paths, so ordering, spans and
    /// diagnostics all work exactly as they do for real files.
    pub embedded: bool,
    /// Set on a module that is an imported C or C++ header (spec §15.1). Its declarations
    /// come from libclang rather than from Dazzle source, and nothing is generated for it:
    /// its names are the header's own.
    pub foreign: Option<Foreign>,
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
    /// Make an imported header a module of the workspace, and a dependency of `importer`
    /// under the key `dep` gave it (spec §15.1). A header two modules import with the same
    /// flags is one module: the same C declarations, named once.
    pub fn add_foreign(&mut self, importer: ModuleId, dep: &CppDep, foreign: Foreign) -> ModuleId {
        let existing = self
            .modules
            .iter()
            .find(|m| {
                m.foreign
                    .as_ref()
                    .is_some_and(|f| std::sync::Arc::ptr_eq(&f.header, &foreign.header))
            })
            .map(|m| m.id);
        let id = match existing {
            Some(id) => id,
            None => {
                let id = ModuleId(self.modules.len() as u32);
                let header_path = foreign.header.path.clone();
                self.modules.push(Module {
                    id,
                    name: dep.recipe.clone(),
                    name_span: Span::DUMMY,
                    version: String::new(),
                    root_dir: header_path.parent().map(Path::to_path_buf).unwrap_or_default(),
                    manifest: foreign.files.first().copied().unwrap_or(FileId(0)),
                    manifest_path: header_path,
                    sources: Vec::new(),
                    deps: Vec::new(),
                    cpp_deps: Vec::new(),
                    via_deps: Vec::new(),
                    names: Vec::new(),
                    flags: Vec::new(),
                    build: BuildConfig::default(),
                    cpp: CppConfig::default(),
                    embedded: false,
                    foreign: Some(foreign),
                });
                id
            }
        };
        self.modules[importer.0 as usize].deps.push(Dependency {
            alias: dep.alias.clone(),
            alias_span: dep.alias_span,
            module: id,
            implicit: false,
            flags: Vec::new(),
            open: dep.open,
            public: dep.public,
            source_span: dep.recipe_span,
        });
        id
    }
}

impl Module {
    /// Look up a dependency by the alias used in `import`.
    pub fn dependency(&self, alias: &str) -> Option<&Dependency> {
        self.deps.iter().find(|d| d.alias == alias)
    }

    /// The bytes of one of this module's sources. Every phase goes through here rather
    /// than calling `std::fs::read`, so the standard library needs no special case
    /// downstream.
    pub fn read_source(&self, path: &Path) -> std::io::Result<Vec<u8>> {
        if self.embedded {
            return std_source(path).map(|t| t.as_bytes().to_vec()).ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::NotFound, "not a standard-library file")
            });
        }
        std::fs::read(path)
    }

    /// Is this the standard library (spec §14.4)? It is emitted like any other module —
    /// `Ordering` and `String` are real types — so this is for diagnostics and for what an
    /// editor may change, not for skipping work.
    pub fn is_prelude(&self) -> bool {
        self.embedded
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
pub fn load(entry: &Path, map: &mut SourceMap, diags: &mut Diagnostics) -> Option<Workspace> {
    let manifest_path =
        normalize(&if entry.is_dir() { entry.join("dazzle.toml") } else { entry.to_owned() });

    let mut loader = Loader { map, diags, modules: Vec::new(), by_path: HashMap::new() };
    let prelude = loader.add_prelude();
    let root = loader.load_module(&manifest_path, None)?;

    // Every module depends on the standard library, because the language's own constructs
    // are written in it (spec §14.4). One whose manifest does not list it gets the
    // dependency anyway, marked implicit, which name resolution treats as not nameable.
    let mut modules = loader.modules;
    for m in &mut modules {
        if m.id != prelude && !m.deps.iter().any(|d| d.module == prelude) {
            m.deps.push(Dependency {
                alias: STD_MODULE.to_owned(),
                alias_span: Span::DUMMY,
                module: prelude,
                implicit: true,
                flags: Vec::new(),
                open: false,
                public: false,
                source_span: Span::DUMMY,
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

/// The standard library's module name, and the dependency key a manifest lists it under
/// (spec §3.2, §14.4). Reserved: no project may be called this.
pub const STD_MODULE: &str = "std";

/// The standard library (spec §14.4): one module, carried by the compiler as Dazzle source,
/// one declaration to a file. Each entry is the pseudo-path the file is shown under — it
/// names nothing on disk — and its text. Folders are organisation only, as in any project
/// (§3.1). A test holds this list to the files under `src/std`.
pub const STD_SOURCES: &[(&str, &str)] = &[
    ("<std>/collections/hash_map.dz", include_str!("std/collections/hash_map.dz")),
    ("<std>/collections/hash_map_iter.dz", include_str!("std/collections/hash_map_iter.dz")),
    (
        "<std>/collections/hash_map_iter_mut.dz",
        include_str!("std/collections/hash_map_iter_mut.dz"),
    ),
    ("<std>/collections/list.dz", include_str!("std/collections/list.dz")),
    ("<std>/collections/slice_iter.dz", include_str!("std/collections/slice_iter.dz")),
    ("<std>/collections/sort.dz", include_str!("std/collections/sort.dz")),
    ("<std>/collections/slice_iter_mut.dz", include_str!("std/collections/slice_iter_mut.dz")),
    ("<std>/core/assert.dz", include_str!("std/core/assert.dz")),
    ("<std>/core/format.dz", include_str!("std/core/format.dz")),
    ("<std>/core/formatter.dz", include_str!("std/core/formatter.dz")),
    ("<std>/core/hash.dz", include_str!("std/core/hash.dz")),
    ("<std>/core/iterate.dz", include_str!("std/core/iterate.dz")),
    ("<std>/core/ops.dz", include_str!("std/core/ops.dz")),
    ("<std>/core/ordering.dz", include_str!("std/core/ordering.dz")),
    ("<std>/core/ownership.dz", include_str!("std/core/ownership.dz")),
    ("<std>/io/directory.dz", include_str!("std/io/directory.dz")),
    ("<std>/io/file.dz", include_str!("std/io/file.dz")),
    ("<std>/io/file_mode.dz", include_str!("std/io/file_mode.dz")),
    ("<std>/io/io_error.dz", include_str!("std/io/io_error.dz")),
    ("<std>/io/native.dz", include_str!("std/io/native.dz")),
    ("<std>/io/path.dz", include_str!("std/io/path.dz")),
    ("<std>/io/process.dz", include_str!("std/io/process.dz")),
    ("<std>/math/constants.dz", include_str!("std/math/constants.dz")),
    ("<std>/math/float.dz", include_str!("std/math/float.dz")),
    ("<std>/math/integer.dz", include_str!("std/math/integer.dz")),
    ("<std>/math/mat4.dz", include_str!("std/math/mat4.dz")),
    ("<std>/math/number.dz", include_str!("std/math/number.dz")),
    ("<std>/math/quat.dz", include_str!("std/math/quat.dz")),
    ("<std>/math/vec2.dz", include_str!("std/math/vec2.dz")),
    ("<std>/math/vec2i.dz", include_str!("std/math/vec2i.dz")),
    ("<std>/math/vec3.dz", include_str!("std/math/vec3.dz")),
    ("<std>/math/vec3i.dz", include_str!("std/math/vec3i.dz")),
    ("<std>/math/vec4.dz", include_str!("std/math/vec4.dz")),
    ("<std>/memory/allocation.dz", include_str!("std/memory/allocation.dz")),
    ("<std>/memory/allocator.dz", include_str!("std/memory/allocator.dz")),
    ("<std>/memory/allocators.dz", include_str!("std/memory/allocators.dz")),
    ("<std>/memory/box.dz", include_str!("std/memory/box.dz")),
    ("<std>/memory/heap.dz", include_str!("std/memory/heap.dz")),
    ("<std>/memory/nullable.dz", include_str!("std/memory/nullable.dz")),
    ("<std>/memory/poison.dz", include_str!("std/memory/poison.dz")),
    ("<std>/memory/rc.dz", include_str!("std/memory/rc.dz")),
    ("<std>/memory/rc_block.dz", include_str!("std/memory/rc_block.dz")),
    ("<std>/memory/temp_arena.dz", include_str!("std/memory/temp_arena.dz")),
    ("<std>/memory/tracker.dz", include_str!("std/memory/tracker.dz")),
    ("<std>/memory/unguarded.dz", include_str!("std/memory/unguarded.dz")),
    ("<std>/memory/weak.dz", include_str!("std/memory/weak.dz")),
    ("<std>/random/random.dz", include_str!("std/random/random.dz")),
    ("<std>/random/random_state.dz", include_str!("std/random/random_state.dz")),
    ("<std>/reflect/field_info.dz", include_str!("std/reflect/field_info.dz")),
    ("<std>/reflect/type_id.dz", include_str!("std/reflect/type_id.dz")),
    ("<std>/reflect/type_info.dz", include_str!("std/reflect/type_info.dz")),
    ("<std>/reflect/type_kind.dz", include_str!("std/reflect/type_kind.dz")),
    ("<std>/reflect/variant_info.dz", include_str!("std/reflect/variant_info.dz")),
    ("<std>/reflect/visibility.dz", include_str!("std/reflect/visibility.dz")),
    ("<std>/text/chars.dz", include_str!("std/text/chars.dz")),
    ("<std>/text/string.dz", include_str!("std/text/string.dz")),
    ("<std>/text/text.dz", include_str!("std/text/text.dz")),
    ("<std>/time/duration.dz", include_str!("std/time/duration.dz")),
    ("<std>/time/instant.dz", include_str!("std/time/instant.dz")),
    ("<std>/time/native.dz", include_str!("std/time/native.dz")),
    ("<std>/time/time.dz", include_str!("std/time/time.dz")),
];

/// The text of the standard-library file shown at `pseudo`, if it is one.
pub fn std_source(pseudo: &Path) -> Option<&'static str> {
    let wanted = pseudo.to_string_lossy().replace('\\', "/");
    STD_SOURCES.iter().find(|(p, _)| *p == wanted).map(|(_, text)| *text)
}

/// Every source the compiler carries rather than reads, as `(pseudo-path, text)`. The
/// language server and a debugger are shown copies of these.
pub fn embedded_sources() -> Vec<(&'static str, &'static str)> {
    STD_SOURCES.to_vec()
}

/// What one manifest's `[dependencies]` declared, by kind.
#[derive(Default)]
struct LoadedDeps {
    deps: Vec<Dependency>,
    cpp: Vec<CppDep>,
    via: Vec<ViaDep>,
}

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
        let sources: Vec<PathBuf> = STD_SOURCES.iter().map(|(p, _)| PathBuf::from(p)).collect();
        // A module needs a "manifest" file for diagnostics about it; the standard library has
        // none, so it is the first source, under its own pseudo-path.
        let (first, text) = STD_SOURCES[0];
        let file = self.map.add(PathBuf::from(first), text.to_owned());
        self.modules.push(Module {
            id,
            name: STD_MODULE.to_owned(),
            name_span: Span::DUMMY,
            version: env!("CARGO_PKG_VERSION").to_owned(),
            root_dir: PathBuf::from("<std>"),
            manifest: file,
            manifest_path: PathBuf::from(first),
            sources,
            deps: Vec::new(),
            cpp_deps: Vec::new(),
            via_deps: Vec::new(),
            names: Vec::new(),
            // The stdlib's own: the allocators' memory tracking (spec §14.2).
            flags: vec![FlagDecl {
                name: "TRACK_MEMORY".to_owned(),
                span: Span::DUMMY,
                default: false,
                doc: Some("record every allocation and report leaks and bad frees".to_owned()),
            }],
            build: BuildConfig::default(),
            cpp: CppConfig::default(),
            embedded: true,
            foreign: None,
        });
        id
    }

    /// `from` is the manifest that depends on this one, used for the cycle message.
    fn load_module(
        &mut self,
        manifest_path: &Path,
        from: Option<(FileId, Span)>,
    ) -> Option<ModuleId> {
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
                    None => self.diags.push(Diagnostic::error(codes::MANIFEST_MISSING, msg)),
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
        let flags = self.flag_decls(file, &doc);
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
            cpp_deps: Vec::new(),
            via_deps: Vec::new(),
            names: Vec::new(),
            flags,
            build,
            cpp,
            embedded: false,
            foreign: None,
        });

        let loaded = self.load_dependencies(file, &doc, &root_dir, id);
        let names = self.name_picks(file, &doc, &loaded);
        let module = &mut self.modules[id.0 as usize];
        module.deps = loaded.deps;
        module.cpp_deps = loaded.cpp;
        module.via_deps = loaded.via;
        module.names = names;

        Some(id)
    }

    fn module_name(&mut self, file: FileId, doc: &Document) -> (String, Span) {
        let Some(table) = doc.table("module") else {
            self.diags.push(
                Diagnostic::error(
                    codes::MANIFEST_MISSING_FIELD,
                    "manifest has no `[module]` table",
                )
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
        } else if name == STD_MODULE || name.starts_with(&format!("{STD_MODULE}::")) {
            // `std` must always mean the compiler's (spec §14.4).
            self.diags.push(
                Diagnostic::error(
                    codes::MANIFEST_BAD_VALUE,
                    format!("`{name}` is reserved for the standard library"),
                )
                .at(file, *span, "a reserved name")
                .with_help("`std` and every name under it belong to the standard library; choose another name (spec §14.4)"),
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
    ) -> LoadedDeps {
        let mut out = LoadedDeps::default();
        let Some(table) = doc.table("dependencies") else { return out };
        let mut seen: Vec<(String, Span)> = Vec::new();

        for entry in &table.entries {
            let key = entry.key.as_str();
            let Value::InlineTable(fields, span) = &entry.value else {
                self.type_error(
                    file,
                    &entry.value,
                    &format!("dependencies.{key}"),
                    "a table like { path = \"../libs/math\" }",
                );
                continue;
            };

            // The key is written in code, `key::Name`, so it must be an identifier.
            let valid = key.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_')
                && key.chars().all(|c| c.is_alphanumeric() || c == '_');
            if !valid {
                self.diags.push(
                    Diagnostic::error(
                        codes::MANIFEST_BAD_VALUE,
                        format!("`{key}` cannot name a dependency"),
                    )
                    .at(file, entry.key_span, "not an identifier")
                    .with_help(
                        "the key is how code writes the dependency, `key::Name`, so it is an \
                         identifier: `rl`, `engine` (spec §3.2)",
                    ),
                );
                continue;
            }
            if let Some((_, prev_span)) = seen.iter().find(|(k, _)| k == key) {
                self.diags.push(
                    Diagnostic::error(
                        codes::MANIFEST_BAD_VALUE,
                        format!("duplicate dependency `{key}`"),
                    )
                    .at(file, entry.key_span, "declared twice")
                    .with_label(dz_diag::Label::secondary(
                        file,
                        *prev_span,
                        "first declared here",
                    )),
                );
                continue;
            }
            seen.push((key.to_owned(), entry.key_span));

            let open = self.bool_field(file, key, fields, "open");
            let public = self.bool_field(file, key, fields, "pub");
            const KEYS: &[&str] = &["path", "cpp", "via", "open", "pub", "flags"];
            for f in fields {
                if !KEYS.contains(&f.key.as_str()) {
                    self.diags.push(
                        Diagnostic::error(
                            codes::MANIFEST_BAD_VALUE,
                            format!("unknown key `{}` in dependency `{key}`", f.key),
                        )
                        .at(file, f.key_span, "not a key of a dependency")
                        .with_help(
                            "a dependency takes one of `path`, `cpp` or `via`, and `open`, \
                             `pub` and `flags` (spec §3.2)",
                        ),
                    );
                }
            }
            let source: Vec<&toml::Entry> = fields
                .iter()
                .filter(|f| ["path", "cpp", "via"].contains(&f.key.as_str()))
                .collect();

            // `std = { open = true }`: the compiler's own, found without a path (§14.4).
            if key == STD_MODULE {
                if let Some(f) = source.first() {
                    self.diags.push(
                        Diagnostic::error(
                            codes::MANIFEST_BAD_VALUE,
                            format!("`std` is the standard library and takes no `{}`", f.key),
                        )
                        .at(file, f.key_span, "the compiler carries it")
                        .with_help("write `std = { open = true }`, or `std = {}` to write `std::Name` (spec §14.4)"),
                    );
                }
                out.deps.push(Dependency {
                    alias: key.to_owned(),
                    alias_span: entry.key_span,
                    module: ModuleId(0),
                    implicit: false,
                    flags: Vec::new(),
                    open,
                    public,
                    source_span: entry.key_span,
                });
                continue;
            }

            let [source] = source[..] else {
                let (msg, label) = if source.is_empty() {
                    (
                        format!("dependency `{key}` says nothing about where it comes from"),
                        "expected `path`, `cpp` or `via`",
                    )
                } else {
                    (
                        format!("dependency `{key}` names more than one source"),
                        "one of `path`, `cpp` or `via`",
                    )
                };
                self.diags.push(
                    Diagnostic::error(codes::MANIFEST_MISSING_FIELD, msg)
                        .at(file, *span, label)
                        .with_help(
                            "`path = \"../libs/math\"` for a project, `cpp = \"raylib\"` for a \
                             `[cpp.libs]` recipe's header, `via = \"rl::shapes\"` for a public \
                             dependency of a dependency (spec §3.2)",
                        ),
                );
                continue;
            };
            let Value::Str(value, value_span) = &source.value else {
                self.type_error(
                    file,
                    &source.value,
                    &format!("dependencies.{key}.{}", source.key),
                    "a string",
                );
                continue;
            };
            match source.key.as_str() {
                "cpp" => {
                    out.cpp.push(CppDep {
                        alias: key.to_owned(),
                        alias_span: entry.key_span,
                        recipe: value.clone(),
                        recipe_span: *value_span,
                        open,
                        public,
                    });
                    continue;
                }
                "via" => {
                    let path: Vec<String> = value.split("::").map(str::to_owned).collect();
                    if path.len() < 2 || path.iter().any(|s| s.is_empty()) {
                        self.diags.push(
                            Diagnostic::error(
                                codes::MANIFEST_BAD_VALUE,
                                format!("`{value}` is not a path to a dependency's dependency"),
                            )
                            .at(file, *value_span, "expected `key::name`")
                            .with_help("for example `via = \"rl::shapes\"` (spec §3.2)"),
                        );
                        continue;
                    }
                    out.via.push(ViaDep {
                        alias: key.to_owned(),
                        alias_span: entry.key_span,
                        path,
                        path_span: *value_span,
                        open,
                        public,
                    });
                    continue;
                }
                _ => {}
            }
            let (rel, path_span) = (value, value_span);

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
                            .at(
                                file,
                                *path_span,
                                "this points back at this module",
                            ),
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
                    out.deps.push(Dependency {
                        alias: entry.key.clone(),
                        alias_span: entry.key_span,
                        module: dep_id,
                        implicit: false,
                        flags: self.dependency_flags(file, &entry.key, fields),
                        open,
                        public,
                        source_span: *path_span,
                    });
                }
                None => continue,
            }
        }
        out
    }

    /// `open = true` or `pub = true` in a dependency; absent is false.
    fn bool_field(&mut self, file: FileId, dep: &str, fields: &[toml::Entry], key: &str) -> bool {
        match fields.iter().find(|f| f.key == key).map(|f| &f.value) {
            None => false,
            Some(Value::Bool(b, _)) => *b,
            Some(other) => {
                self.type_error(file, other, &format!("dependencies.{dep}.{key}"), "true or false");
                false
            }
        }
    }

    /// `[names]`: `Vec2 = "std"` — which dependency a bare name means (spec §3.2).
    fn name_picks(&mut self, file: FileId, doc: &Document, deps: &LoadedDeps) -> Vec<NamePick> {
        let Some(table) = doc.table("names") else { return Vec::new() };
        let mut out: Vec<NamePick> = Vec::new();
        for entry in &table.entries {
            let Value::Str(dep, dep_span) = &entry.value else {
                self.type_error(
                    file,
                    &entry.value,
                    &format!("names.{}", entry.key),
                    "a string naming a dependency, like \"std\"",
                );
                continue;
            };
            let known = deps
                .deps
                .iter()
                .map(|d| d.alias.as_str())
                .chain(deps.cpp.iter().map(|d| d.alias.as_str()))
                .chain(deps.via.iter().map(|d| d.alias.as_str()))
                .any(|a| a == dep);
            if !known {
                self.diags.push(
                    Diagnostic::error(
                        codes::MANIFEST_BAD_VALUE,
                        format!("`{dep}` is not a dependency of this module"),
                    )
                    .at(file, *dep_span, "not in `[dependencies]`")
                    .with_help(
                        "`[names]` chooses among this module's dependencies, by key (spec §3.2)",
                    ),
                );
                continue;
            }
            out.push(NamePick {
                name: entry.key.clone(),
                name_span: entry.key_span,
                dep: dep.clone(),
                dep_span: *dep_span,
            });
        }
        out
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

    /// `[flags]`: each entry a flag this package reads with `#if` (spec §12.10).
    fn flag_decls(&mut self, file: FileId, doc: &Document) -> Vec<FlagDecl> {
        let Some(table) = doc.table("flags") else { return Vec::new() };
        let mut out: Vec<FlagDecl> = Vec::new();
        for entry in &table.entries {
            let name = entry.key.clone();
            let valid = name.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_')
                && name.chars().all(|c| c.is_alphanumeric() || c == '_');
            if !valid {
                self.diags.push(
                    Diagnostic::error(
                        codes::MANIFEST_BAD_VALUE,
                        format!("`{name}` is not a valid flag name"),
                    )
                    .at(file, entry.key_span, "not an identifier")
                    .with_help("a flag is named like a constant: `TRACK_MEMORY` (spec §12.10)"),
                );
                continue;
            }
            if BUILTIN_FLAGS.contains(&name.as_str()) {
                self.diags.push(
                    Diagnostic::error(
                        codes::MANIFEST_BAD_VALUE,
                        format!("`{name}` is a flag the compiler already defines"),
                    )
                    .at(file, entry.key_span, "built in")
                    .with_help(
                        "`DEBUG`, `RELEASE`, `SHIPPING` and the `OS_…` flags come from the build \
                         mode and the platform, in every package; name this one differently \
                         (spec §12.10)",
                    ),
                );
                continue;
            }
            let (default, doc_text) = match &entry.value {
                Value::Bool(b, _) => (*b, None),
                Value::InlineTable(fields, _) => {
                    let mut default = false;
                    let mut doc_text = None;
                    for f in fields {
                        match (f.key.as_str(), &f.value) {
                            ("default", Value::Bool(b, _)) => default = *b,
                            ("doc", Value::Str(t, _)) => doc_text = Some(t.clone()),
                            ("default", other) => self.type_error(
                                file,
                                other,
                                &format!("flags.{name}.default"),
                                "true or false",
                            ),
                            ("doc", other) => self.type_error(
                                file,
                                other,
                                &format!("flags.{name}.doc"),
                                "a string",
                            ),
                            (key, _) => self.diags.push(
                                Diagnostic::error(
                                    codes::MANIFEST_BAD_VALUE,
                                    format!("unknown key `{key}` in flag `{name}`"),
                                )
                                .at(file, f.key_span, "not a key of a flag")
                                .with_help("a flag takes `default` and `doc` (spec §12.10)"),
                            ),
                        }
                    }
                    (default, doc_text)
                }
                other => {
                    self.type_error(
                        file,
                        other,
                        &format!("flags.{name}"),
                        "true, false, or a table like { default = false, doc = \"…\" }",
                    );
                    continue;
                }
            };
            if out.iter().any(|f| f.name == name) {
                self.diags.push(
                    Diagnostic::error(
                        codes::MANIFEST_BAD_VALUE,
                        format!("flag `{name}` is declared twice"),
                    )
                    .at(file, entry.key_span, "declared again"),
                );
                continue;
            }
            out.push(FlagDecl { name, span: entry.key_span, default, doc: doc_text });
        }
        out
    }

    /// A dependency's `flags = ["PROFILE"]`. Whether each names a flag the dependency
    /// declares is checked once every module is loaded (spec §12.10).
    fn dependency_flags(
        &mut self,
        file: FileId,
        alias: &str,
        fields: &[toml::Entry],
    ) -> Vec<(String, Span)> {
        let Some(entry) = fields.iter().find(|f| f.key == "flags") else { return Vec::new() };
        let Value::Array(items, _) = &entry.value else {
            self.type_error(
                file,
                &entry.value,
                &format!("dependencies.{alias}.flags"),
                "an array of flag names",
            );
            return Vec::new();
        };
        let mut out = Vec::new();
        for item in items {
            match item {
                Value::Str(name, span) => out.push((name.clone(), *span)),
                other => self.type_error(
                    file,
                    other,
                    &format!("dependencies.{alias}.flags entry"),
                    "a string",
                ),
            }
        }
        out
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
        if let Some(v) = table.get("cxx") {
            match as_str(v) {
                Some(s) => cfg.cxx = Some(s.to_owned()),
                None => self.type_error(file, v, "build.cxx", "a string"),
            }
        }
        // `[build.target.<os>]` replaces what the base table said, for this platform only.
        // Replaces rather than adds: there is one compiler, not a list of them.
        let key = format!("build.target.{}", host_target());
        if let Some(v) = doc.table(&key).and_then(|t| t.get("cxx")) {
            match as_str(v) {
                Some(s) => cfg.cxx = Some(s.to_owned()),
                None => self.type_error(file, v, &format!("{key}.cxx"), "a string"),
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
        if let Some(v) = table.get("track_memory") {
            match v {
                Value::Bool(b, _) => cfg.track_memory = *b,
                other => self.type_error(file, other, "build.track_memory", "true or false"),
            }
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
        const MODULE_KEYS: &[&str] =
            &["include_dirs", "defines", "lib_dirs", "link_libs", "frameworks"];
        const LIB_KEYS: &[&str] = &[
            "root",
            "sources",
            "header",
            "include_dirs",
            "defines",
            "lib_dirs",
            "link_libs",
            "frameworks",
            "flags",
            "import",
        ];
        const LIB_TARGET_KEYS: &[&str] = &[
            "sources",
            "include_dirs",
            "defines",
            "lib_dirs",
            "link_libs",
            "frameworks",
            "flags",
            "import",
        ];
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
                ["cpp"] => {
                    self.cpp_section(file, table, root_dir, MANIFEST_DIR, MODULE_KEYS, &mut module)
                }
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
            frameworks: module.frameworks,
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
                    frameworks: l.frameworks,
                    flags: l.flags,
                    import: l.import,
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
                self.type_error(
                    file,
                    &entry.value,
                    &format!("{}.{key}", table.name),
                    "an array of strings",
                );
                continue;
            };
            for item in items {
                let Value::Str(s, span) = item else {
                    self.diags.push(
                        Diagnostic::error(
                            codes::MANIFEST_BAD_VALUE,
                            format!("`{}.{key}` entries must be strings", table.name),
                        )
                        .at(
                            file,
                            item.span(),
                            format!("this is {}", item.type_name()),
                        ),
                    );
                    continue;
                };
                match key {
                    "defines" => out.defines.push(s.clone()),
                    "link_libs" => out.link_libs.push(s.clone()),
                    "frameworks" => out.frameworks.push(s.clone()),
                    "flags" => out.flags.push(s.clone()),
                    // Found through the include dirs when the header is read, as
                    // `#include <…>` finds it (spec §15.1), so not a path to check here.
                    "import" => out.import.push((s.clone(), *span)),
                    "sources" => {
                        self.lib_sources(file, base, base_desc, s, *span, &mut out.sources)
                    }
                    _ => {
                        let dir = normalize(&base.join(s));
                        if !dir.is_dir() {
                            self.missing_dir(file, s, *span, base_desc);
                        }
                        let list = if key == "include_dirs" {
                            &mut out.include_dirs
                        } else {
                            &mut out.lib_dirs
                        };
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
                    Diagnostic::error(
                        codes::MANIFEST_BAD_VALUE,
                        format!("invalid source pattern: {e}"),
                    )
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
            Diagnostic::error(
                codes::MANIFEST_BAD_VALUE,
                format!("directory `{path}` does not exist"),
            )
            .at(file, span, "no such directory")
            .with_help(format!("the path is relative to {base_desc}")),
        );
    }

    fn type_error(&mut self, file: FileId, value: &Value, what: &str, expected: &str) {
        self.diags.push(
            Diagnostic::error(codes::MANIFEST_BAD_VALUE, format!("`{what}` must be {expected}"))
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
    frameworks: Vec<String>,
    flags: Vec<String>,
    import: Vec<(String, Span)>,
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
                let popped =
                    matches!(out.components().next_back(), Some(std::path::Component::Normal(_)))
                        && out.pop();
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
