//! Name resolution and visibility (spec §3, §4).
//!
//! What this phase decides:
//!
//! * which declaration every `::` path refers to,
//! * whether that declaration is visible from where it is named,
//! * that no module declares the same name twice,
//! * that every local binding exists before it is used.
//!
//! What it deliberately does **not** do: resolve `.field` and `.method()`. Those need the
//! type of the receiver, which is M3's job. `::` is the compile-time operator and `.` is
//! the runtime one (spec §3.3), and that split falls out exactly along the M2/M3 line.

use std::collections::{HashMap, HashSet};

use dz_ast::*;
use dz_diag::{codes, Diagnostic, Diagnostics};
use dz_project::{Foreign, ModuleId, Workspace};
use dz_span::{FileId, Span};

mod walk;

/// A parsed source file belonging to some module.
pub struct ParsedFile {
    pub file: FileId,
    pub ast: SourceFileAst,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct SymbolId(pub u32);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SymbolKind {
    Struct,
    Union,
    Enum,
    Trait,
    Fn,
    Const,
    /// A `static`: one variable for the program, or per thread (spec §7.7).
    Static,
    Macro,
    /// A C++ namespace, which holds the declarations written inside it: Dazzle reaches
    /// them through it, `eng::ui::draw` (spec §15.2).
    Namespace,
}

impl SymbolKind {
    pub fn describe(&self) -> &'static str {
        match self {
            SymbolKind::Struct => "struct",
            SymbolKind::Union => "union",
            SymbolKind::Enum => "enum",
            SymbolKind::Trait => "trait",
            SymbolKind::Fn => "function",
            SymbolKind::Const => "constant",
            SymbolKind::Static => "static",
            SymbolKind::Macro => "macro",
            SymbolKind::Namespace => "namespace",
        }
    }

    /// [`SymbolKind::describe`] with its article, for a message that reads as English.
    /// `enum` is the only vowel-initial kind, but spelling the rule out is cheaper than
    /// remembering the exception at every call site.
    pub fn describe_a(&self) -> String {
        let word = self.describe();
        let article = match word.chars().next() {
            Some('a' | 'e' | 'i' | 'o' | 'u') => "an",
            _ => "a",
        };
        format!("{article} {word}")
    }

    /// Can this be named on the left of `::`?
    pub fn has_members(&self) -> bool {
        matches!(
            self,
            SymbolKind::Struct
                | SymbolKind::Union
                | SymbolKind::Enum
                | SymbolKind::Trait
                | SymbolKind::Namespace
        )
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MemberSort {
    Field,
    Method,
    Ctor,
    Variant,
    AssocType,
    /// `const Global` inside a type (spec §7.3).
    Const,
}

impl MemberSort {
    fn describe(&self) -> &'static str {
        match self {
            MemberSort::Field => "field",
            MemberSort::Method => "method",
            MemberSort::Ctor => "constructor",
            MemberSort::Variant => "variant",
            MemberSort::AssocType => "associated type",
            MemberSort::Const => "constant",
        }
    }
}

#[derive(Clone, Debug)]
pub struct MemberSymbol {
    pub name: String,
    pub sort: MemberSort,
    pub vis: Vis,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    pub vis: Vis,
    pub module: ModuleId,
    pub file: FileId,
    /// Span of the declared name, which is what diagnostics point at.
    pub span: Span,
    pub members: Vec<MemberSymbol>,
    /// `struct Button : UiElement` — filled in between the two passes, because the base is
    /// a path and paths are not resolvable until every declaration exists. The resolver
    /// needs it so a derived type's methods see the base's fields unqualified, and the
    /// editor needs it for the same reason (spec §7.3, 18.32).
    pub base: Option<SymbolId>,
}

impl Symbol {
    pub fn member(&self, name: &str) -> Option<&MemberSymbol> {
        self.members.iter().find(|m| m.name == name)
    }
}

impl Resolution {
    /// A struct's own members and every base's, nearest declaration last so a walk that
    /// takes the first match gets the most-derived one. Cycle-safe: a chain that comes
    /// back round stops, and `dz_check` is what reports it (spec §7.3).
    pub fn members_with_bases(&self, sym: SymbolId) -> Vec<&MemberSymbol> {
        let mut chain = Vec::new();
        let mut at = Some(sym);
        while let Some(s) = at {
            if chain.contains(&s) {
                break;
            }
            chain.push(s);
            at = self.symbol(s).base;
        }
        // Bases first, so a derived declaration of the same name comes later and wins.
        chain.iter().rev().flat_map(|s| self.symbol(*s).members.iter()).collect()
    }
}

/// Everything declared at module scope, for one module.
#[derive(Default, Debug)]
pub struct ModuleScope {
    pub items: HashMap<String, SymbolId>,
}

/// What one file's `import` lines brought into scope.
///
/// Recorded so the type checker resolves a leading path segment exactly the way name
/// resolution did, instead of reimplementing the rules and drifting from them.
#[derive(Default, Debug)]
pub struct FileImports {
    /// `import engine::render as gfx` → "gfx".
    pub module_aliases: HashMap<String, ModuleId>,
    /// `import math::{ pow }` → "pow".
    pub unqualified: HashMap<String, SymbolId>,
}

/// What a module's `dazzle.toml` lets every one of its files name (spec §3.2).
#[derive(Default, Debug)]
pub struct ModuleNames {
    /// Each nameable dependency by its key: `path`, `cpp` and `via` ones, and `std` when
    /// listed.
    pub deps: HashMap<String, ModuleId>,
    /// The dependencies with `open = true`, whose exports are also written bare.
    pub opened: Vec<ModuleId>,
    /// `[names]`: a bare name, and the declaration it was chosen to mean.
    pub picks: HashMap<String, SymbolId>,
    /// The dependencies with `pub = true`: what a dependent reaches as `key::name`.
    pub public: HashMap<String, ModuleId>,
}

/// What a bare name finds among a file's opened modules.
pub enum Opened {
    None,
    One(SymbolId),
    /// Two or more different declarations: the name has to be written with its module.
    Ambiguous(Vec<SymbolId>),
}

pub struct Resolution {
    pub symbols: Vec<Symbol>,
    pub scopes: HashMap<ModuleId, ModuleScope>,
    pub imports: HashMap<FileId, FileImports>,
    pub names: HashMap<ModuleId, ModuleNames>,
    /// Each dependency key some part of its module went through — a path, an import, an
    /// opened name, `[names]`, a `via` — so one nothing uses can be reported (spec §3.2).
    pub used_deps: HashSet<(ModuleId, String)>,
    /// Which module each source file belongs to. Needed to answer "how would this type be
    /// written *here*", which is what an editor has to show: an imported header's type is
    /// `rl::Vector2` in the file that imported it and has no other spelling (§3.3).
    pub file_module: HashMap<FileId, ModuleId>,
    /// What each C++ namespace symbol holds, by name — `eng::ui::draw` walks one link of
    /// this per segment (spec §15.2).
    pub inner: HashMap<(SymbolId, String), SymbolId>,
    /// Every imported declaration by the C++ path it was imported under, so a type named
    /// in a signature — `::JPH::Body` — finds the symbol it belongs to wherever the
    /// namespaces put it (spec §15.2).
    pub foreign_cpp: HashMap<(ModuleId, String), SymbolId>,
    /// The same in reverse: what the generated C++ writes for an imported symbol.
    pub foreign_path: HashMap<SymbolId, String>,
    /// The prelude (spec §3.2, §18.23), so an unqualified lookup can fall back to it.
    pub prelude: ModuleId,
    pub diagnostics: Diagnostics,
}

impl Resolution {
    pub fn symbol(&self, id: SymbolId) -> &Symbol {
        &self.symbols[id.0 as usize]
    }

    pub fn lookup(&self, module: ModuleId, name: &str) -> Option<SymbolId> {
        self.scopes.get(&module)?.items.get(name).copied()
    }

    /// A name declared inside a C++ namespace (spec §15.2).
    pub fn inner(&self, ns: SymbolId, name: &str) -> Option<SymbolId> {
        self.inner.get(&(ns, name.to_owned())).copied()
    }

    /// The symbol an imported header gave the C++ path `cpp` (spec §15.2).
    pub fn foreign(&self, module: ModuleId, cpp: &str) -> Option<SymbolId> {
        self.foreign_cpp.get(&(module, cpp.to_owned())).copied()
    }

    /// What the generated C++ writes for an imported symbol: `::InitWindow`, `::JPH::Body`,
    /// and for a macro its bare name, which cannot take a `::` (spec §15.2).
    pub fn cpp_path(&self, sym: SymbolId) -> Option<&str> {
        self.foreign_path.get(&sym).map(String::as_str)
    }

    /// Resolve a bare name the way [`walk`] does, for later phases (spec §3.2): the module's
    /// own declarations, the file's `{ … }` imports, the manifest's `[names]`, then the
    /// opened dependencies. The two must agree, or the checker would type a name that
    /// resolution pointed elsewhere.
    pub fn lookup_in_file(&self, module: ModuleId, file: FileId, name: &str) -> Option<SymbolId> {
        if let Some(id) = self.lookup(module, name) {
            return Some(id);
        }
        if let Some(id) = self.imports.get(&file).and_then(|i| i.unqualified.get(name)) {
            return Some(*id);
        }
        let names = self.names.get(&module)?;
        if let Some(id) = names.picks.get(name) {
            return Some(*id);
        }
        match self.find_opened(module, &names.opened, name) {
            Opened::One(id) => Some(id),
            // Resolution reported it; typing it as nothing keeps that the only error.
            Opened::Ambiguous(_) | Opened::None => None,
        }
    }

    /// The module a dependency key names from `file`: an `import … as` of that file first,
    /// then the module's manifest (spec §3.2).
    pub fn module_alias(&self, file: FileId, name: &str) -> Option<ModuleId> {
        if let Some(m) = self.imports.get(&file).and_then(|i| i.module_aliases.get(name)) {
            return Some(*m);
        }
        let module = self.file_module.get(&file)?;
        self.names.get(module)?.deps.get(name).copied()
    }

    /// How many leading segments of a path name modules, and the last module they reach:
    /// `rl::shapes::Rectangle` walks `rl`, then its public dependency `shapes` (spec §3.2).
    /// A declaration of the module wins over a public dependency of the same name.
    pub fn module_prefix(&self, file: FileId, segments: &[&str]) -> Option<(ModuleId, usize)> {
        let mut m = self.module_alias(file, segments.first()?)?;
        let mut used = 1;
        while let Some(seg) = segments.get(used) {
            if self.lookup(m, seg).is_some() {
                break;
            }
            match self.names.get(&m).and_then(|n| n.public.get(*seg)) {
                Some(next) => {
                    m = *next;
                    used += 1;
                }
                None => break,
            }
        }
        Some((m, used))
    }

    /// The declaration a path names before its members, and how many segments that took:
    /// a bare name in scope, or modules then a declaration, then a C++ namespace per
    /// segment (spec §3.2, §15.2). `None` when it names no declaration.
    pub fn path_item(
        &self,
        module: ModuleId,
        file: FileId,
        segments: &[&str],
    ) -> Option<(SymbolId, usize)> {
        let (mut sym, mut used) = match self.module_prefix(file, segments) {
            Some((m, n)) if segments.len() > n => (self.lookup(m, segments[n])?, n + 1),
            _ => (self.lookup_in_file(module, file, segments.first()?)?, 1),
        };
        while self.symbol(sym).kind == SymbolKind::Namespace {
            let Some(seg) = segments.get(used) else { break };
            let Some(next) = self.inner(sym, seg) else { break };
            sym = next;
            used += 1;
        }
        Some((sym, used))
    }

    /// How `file` writes a module: the key or file alias it goes by, or failing that its
    /// declared name.
    pub fn module_spelling(&self, file: FileId, m: ModuleId) -> Option<String> {
        if let Some((k, _)) =
            self.imports.get(&file).and_then(|i| i.module_aliases.iter().find(|(_, v)| **v == m))
        {
            return Some(k.clone());
        }
        let module = self.file_module.get(&file)?;
        let names = self.names.get(module)?;
        let mut keys: Vec<&String> =
            names.deps.iter().filter(|(_, v)| **v == m).map(|(k, _)| k).collect();
        keys.sort();
        keys.first().map(|k| (*k).clone())
    }

    /// A bare name in the modules a file opened with `::*`, seen from `viewer`: only what
    /// each exports, and never a name the language itself provides (spec §3.2). The one
    /// rule both [`walk`] and the checker use, so they cannot disagree.
    pub fn find_opened(&self, viewer: ModuleId, opened: &[ModuleId], name: &str) -> Opened {
        if BUILTIN_TYPES.contains(&name) || BUILTIN_FNS.contains(&name) {
            return Opened::None;
        }
        let mut found: Vec<SymbolId> = Vec::new();
        for &m in opened {
            if m == viewer {
                continue;
            }
            let Some(id) = self.lookup(m, name) else { continue };
            let s = self.symbol(id);
            // A typedef is a second name for the same symbol, so two paths to one
            // declaration are not a conflict.
            if visible_from(s.vis, s.module, viewer) && !found.contains(&id) {
                found.push(id);
            }
        }
        match found.len() {
            0 => Opened::None,
            1 => Opened::One(found[0]),
            _ => Opened::Ambiguous(found),
        }
    }
}

/// Names the language provides without declaration.
///
/// Each is grounded in a specific part of the spec rather than invented here: the
/// primitives are §5.1, `str`/`cstr` are §5.3, `sizeof`/`alignof` are §5.5, `error` is
/// §9.1, `panic` is §9.8, the formatting calls are §12.9, the reflection calls are §13.1,
/// and `construct`/`destroy` are §14.3.
pub const BUILTIN_TYPES: &[&str] = &[
    "i8",
    "i16",
    "i32",
    "i64",
    "u8",
    "u16",
    "u32",
    "u64",
    "isize",
    "usize",
    "f32",
    "f64",
    "bool",
    "char",
    "str",
    "cstr",
    "void",
    "Self",
    // Provided by the language rather than the stdlib (spec §9.2, §12.7).
    "Error",
    "SourceLocation",
];

pub const BUILTIN_FNS: &[&str] = &[
    // No `alloc` or `free`: memory comes from an `Allocator`, and the C heap underneath
    // `Allocators::Global` is `std`'s own import of `stdlib.h` (spec §14.2, r74).
    "sizeof",
    "alignof",
    "construct",
    "destroy",
    "move_out",
    "error",
    "typeinfo_of",
    "typeid_of",
    "typeinfo",
    "memcpy",
    "memmove",
    // Stops the optimiser deleting work whose result nothing reads, which is every
    // benchmark (spec §19 M15).
    "blackbox",
];

/// The compile-time queries every type answers through `::` (spec §5.5), so resolution can
/// accept `T::size` and `Vec3::is_copyable` without mistaking them for members.
/// `dz_hir::TypeQuery::from_name` is the authority on what each one means, and a test there
/// keeps the two lists in step. `T::fields` is absent: it is a compile-time sequence for
/// macros and reflection, which are M10 and M11.
pub const TYPE_QUERIES: &[&str] =
    &["size", "align", "name", "is_trivially_copyable", "is_copyable", "has_drop"];

/// Resolve every module in the workspace.
pub fn resolve(ws: &Workspace, files: &HashMap<ModuleId, Vec<ParsedFile>>) -> Resolution {
    let mut r = Resolution {
        symbols: Vec::new(),
        scopes: HashMap::new(),
        imports: HashMap::new(),
        file_module: HashMap::new(),
        inner: HashMap::new(),
        foreign_cpp: HashMap::new(),
        foreign_path: HashMap::new(),
        names: HashMap::new(),
        used_deps: HashSet::new(),
        prelude: ws.prelude,
        diagnostics: Diagnostics::new(),
    };

    // Pass 1: every module's declarations, so order never matters within a module
    // and cross-module references can be checked in pass 2 (spec §3.2).
    for module in ws.modules() {
        let mut scope = ModuleScope::default();
        if let Some(f) = &module.foreign {
            collect_foreign(&mut r, &mut scope, module.id, f);
        }
        if let Some(module_files) = files.get(&module.id) {
            for pf in module_files {
                collect_file(&mut r, &mut scope, module.id, pf);
            }
        }
        r.scopes.insert(module.id, scope);
    }

    // Pass 1a: what each manifest lets its module name (spec §3.2). After pass 1 because
    // `[names]` names declarations.
    for module in ws.modules() {
        module_names(&mut r, ws, module.id);
    }

    // Pass 1b: link each `struct X : Base` to its base's symbol. Between the passes
    // because a base is a path — no path resolves until every declaration exists — and
    // before pass 2, which needs *every* link already made: a derived type's methods see
    // the base's fields unqualified, and the base may live in a file walked later, or be
    // itself derived (spec §7.3, 18.32).
    //
    // Resolving a path needs the file's imports, which is pass 2's own first step, so this
    // does that step and then discards what it reported — pass 2 reports it for real.
    let before = r.diagnostics.len();
    for module in ws.modules() {
        if let Some(module_files) = files.get(&module.id) {
            for pf in module_files {
                walk::link_bases(&mut r, ws, module.id, pf);
            }
        }
    }
    r.diagnostics.truncate(before);

    // Pass 2: resolve uses.
    for module in ws.modules() {
        if let Some(module_files) = files.get(&module.id) {
            for pf in module_files {
                walk::resolve_file(&mut r, ws, module.id, pf);
            }
        }
    }

    report_unused_dependencies(&mut r, ws, files);

    r.diagnostics.sort_by_position();
    r
}

/// A `[dependencies]` entry that nothing in its module names: not a path through its key,
/// an import, an opened name, a `[names]` choice or a `via` (spec §3.2). A public one is
/// part of the module's interface whether or not the module itself uses it, and `std` is
/// what the language is written in, so neither is reported.
fn report_unused_dependencies(
    r: &mut Resolution,
    ws: &Workspace,
    files: &HashMap<ModuleId, Vec<ParsedFile>>,
) {
    for module in ws.modules() {
        if module.embedded || module.foreign.is_some() || !files.contains_key(&module.id) {
            continue;
        }
        let entries = module
            .deps
            .iter()
            .filter(|d| !d.implicit && !d.public && d.module != ws.prelude)
            .map(|d| (&d.alias, d.alias_span))
            .chain(module.via_deps.iter().filter(|v| !v.public).map(|v| (&v.alias, v.alias_span)));
        let mut unused: Vec<(String, Span)> = Vec::new();
        for (key, span) in entries {
            if !r.used_deps.contains(&(module.id, key.clone())) {
                unused.push((key.clone(), span));
            }
        }
        for (key, span) in unused {
            r.diagnostics.push(
                Diagnostic::warning(
                    codes::UNUSED_DEPENDENCY,
                    format!("dependency `{key}` is never used"),
                )
                .at(module.manifest, span, "nothing in this module names it")
                .with_help(format!(
                    "remove it from `[dependencies]`, or mark it `pub = true` if it is part of \
                     `{}`'s interface (spec §3.2)",
                    module.name
                )),
            );
        }
    }
}

/// Build `module`'s [`ModuleNames`] from its manifest, and its dependencies' first, since a
/// `via` walks their public dependencies. Memoised: each module is built once.
fn module_names(r: &mut Resolution, ws: &Workspace, module: ModuleId) {
    if r.names.contains_key(&module) {
        return;
    }
    let m = ws.module(module);
    for d in &m.deps {
        module_names(r, ws, d.module);
    }
    let mut names = ModuleNames::default();
    for d in m.deps.iter().filter(|d| !d.implicit) {
        names.deps.insert(d.alias.clone(), d.module);
        if d.open && !names.opened.contains(&d.module) {
            names.opened.push(d.module);
        }
        if d.public {
            names.public.insert(d.alias.clone(), d.module);
        }
    }
    for v in &m.via_deps {
        // The dependency a `via` walks through is used by it.
        if let Some(first) = v.path.first() {
            r.used_deps.insert((module, first.clone()));
        }
        let target = walk_via(r, &names, &v.path);
        let Ok(target) = target else {
            let (seg, owner) = target.unwrap_err();
            let mut d = Diagnostic::error(
                codes::UNKNOWN_MODULE,
                format!("`{}` names no dependency", v.path.join("::")),
            )
            .at(m.manifest, v.path_span, format!("`{seg}` is not found here"));
            d = d.with_help(match owner {
                Some(o) => format!(
                    "`via` reaches only what `{o}` marks `pub = true` in its dazzle.toml (spec §3.2)"
                ),
                None => "the path starts at a dependency key of this module (spec §3.2)".to_owned(),
            });
            r.diagnostics.push(d);
            continue;
        };
        names.deps.insert(v.alias.clone(), target);
        if v.open && !names.opened.contains(&target) {
            names.opened.push(target);
        }
        if v.public {
            names.public.insert(v.alias.clone(), target);
        }
    }
    for pick in &m.names {
        let Some(&dep) = names.deps.get(&pick.dep) else { continue };
        let sym = r.lookup(dep, &pick.name).filter(|s| {
            let s = r.symbol(*s);
            visible_from(s.vis, s.module, module)
        });
        r.used_deps.insert((module, pick.dep.clone()));
        match sym {
            Some(s) => {
                names.picks.insert(pick.name.clone(), s);
            }
            None => r.diagnostics.push(
                Diagnostic::error(
                    codes::UNRESOLVED_NAME,
                    format!("`{}` exports no `{}`", pick.dep, pick.name),
                )
                .at(m.manifest, pick.name_span, "not a public declaration of that dependency")
                .with_help("`[names]` chooses a declaration a dependency exports (spec §3.2)"),
            ),
        }
    }
    r.names.insert(module, names);
}

/// `["rl", "shapes"]` from a module whose keys are `names`: the first segment a key, each
/// further one a public dependency of the module before it. On failure, the segment not
/// found and the module it was looked for in.
fn walk_via(
    r: &Resolution,
    names: &ModuleNames,
    path: &[String],
) -> Result<ModuleId, (String, Option<String>)> {
    let first = path.first().ok_or((String::new(), None))?;
    let mut at = *names.deps.get(first).ok_or((first.clone(), None))?;
    let mut owner = first.clone();
    for seg in &path[1..] {
        at = *r
            .names
            .get(&at)
            .and_then(|n| n.public.get(seg))
            .ok_or((seg.clone(), Some(owner.clone())))?;
        owner = seg.clone();
    }
    Ok(at)
}

fn collect_file(r: &mut Resolution, scope: &mut ModuleScope, module: ModuleId, pf: &ParsedFile) {
    r.file_module.insert(pf.file, module);
    for item in &pf.ast.items {
        let Some((name, kind, members)) = describe_item(item) else { continue };
        let id = SymbolId(r.symbols.len() as u32);
        let symbol = Symbol {
            name: name.name.clone(),
            kind,
            vis: item.vis,
            module,
            file: pf.file,
            span: name.span,
            members,
            base: None,
        };

        if let Some(previous) = scope.items.get(&symbol.name).copied() {
            let prev = &r.symbols[previous.0 as usize];
            let prev_span = prev.span;
            let prev_file = prev.file;
            let prev_kind = prev.kind.describe();
            r.diagnostics.push(
                Diagnostic::error(
                    codes::DUPLICATE_DEFINITION,
                    format!("`{}` is defined more than once in this module", symbol.name),
                )
                .at(pf.file, symbol.span, format!("redefined as a {}", kind.describe()))
                .with_label(dz_diag::Label::secondary(
                    prev_file,
                    prev_span,
                    format!("first defined as a {prev_kind} here"),
                ))
                .with_help(
                    "a module is one namespace shared by all of its files (spec §3.1), so two \
                     files cannot declare the same name",
                ),
            );
            r.symbols.push(symbol);
            continue;
        }

        scope.items.insert(symbol.name.clone(), id);
        r.symbols.push(symbol);
    }
}

/// An imported header's declarations (spec §15.1): symbols like any module's, except that
/// they come from libclang, point into the header, and are all public.
fn collect_foreign(r: &mut Resolution, scope: &mut ModuleScope, module: ModuleId, f: &Foreign) {
    use dz_project::cppimport::{DeclKind, Loc};
    let at = |l: Loc| (f.files[l.file as usize], Span::new(l.start as usize, l.end as usize));
    let mut aliases = Vec::new();
    // A C++ namespace becomes a symbol of its own holding what was written inside it, so
    // `eng::ui::draw` walks one per segment (spec §15.2). Built as declarations arrive, and
    // registered in whichever namespace encloses it.
    let mut namespaces: HashMap<Vec<String>, SymbolId> = HashMap::new();
    for d in &f.header.decls {
        let (file, span) = at(d.loc);
        let mut path: Vec<String> = Vec::new();
        let mut parent: Option<SymbolId> = None;
        for segment in &d.scope {
            path.push(segment.clone());
            let id = match namespaces.get(&path) {
                Some(id) => *id,
                None => {
                    let id = SymbolId(r.symbols.len() as u32);
                    r.symbols.push(Symbol {
                        name: segment.clone(),
                        kind: SymbolKind::Namespace,
                        vis: Vis::Public,
                        module,
                        file,
                        span,
                        members: Vec::new(),
                        base: None,
                    });
                    namespaces.insert(path.clone(), id);
                    match parent {
                        Some(p) => {
                            r.inner.insert((p, segment.clone()), id);
                            r.symbols[p.0 as usize].members.push(MemberSymbol {
                                name: segment.clone(),
                                sort: MemberSort::AssocType,
                                vis: Vis::Public,
                                span,
                            });
                        }
                        None => {
                            scope.items.entry(segment.clone()).or_insert(id);
                        }
                    }
                    id
                }
            };
            parent = Some(id);
        }
    }

    for d in &f.header.decls {
        let member = |name: &str, sort: MemberSort, loc: Loc| MemberSymbol {
            name: name.to_owned(),
            sort,
            vis: Vis::Public,
            span: at(loc).1,
        };
        let (kind, members) = match &d.kind {
            DeclKind::Fn { .. } => (SymbolKind::Fn, Vec::new()),
            DeclKind::Const { .. } => (SymbolKind::Const, Vec::new()),
            DeclKind::Struct { fields, .. } => (
                SymbolKind::Struct,
                fields.iter().map(|x| member(&x.name, MemberSort::Field, x.loc)).collect(),
            ),
            DeclKind::Enum { variants, .. } => (
                SymbolKind::Enum,
                variants.iter().map(|v| member(&v.name, MemberSort::Variant, v.loc)).collect(),
            ),
            DeclKind::Alias { target } => {
                aliases.push((d.name.clone(), d.scope.clone(), target.clone()));
                continue;
            }
        };
        let (file, span) = at(d.loc);
        let id = SymbolId(r.symbols.len() as u32);
        r.symbols.push(Symbol {
            name: d.name.clone(),
            kind,
            vis: Vis::Public,
            module,
            file,
            span,
            members,
            base: None,
        });
        // Inside a namespace it is reached through it, and only through it; at the
        // header's own scope it is a name of the module (spec §15.2).
        match namespaces.get(&d.scope) {
            Some(ns) => {
                r.inner.entry((*ns, d.name.clone())).or_insert(id);
                r.symbols[ns.0 as usize].members.push(MemberSymbol {
                    name: d.name.clone(),
                    sort: member_sort(kind),
                    vis: Vis::Public,
                    span,
                });
            }
            None => {
                scope.items.entry(d.name.clone()).or_insert(id);
            }
        }
        r.foreign_cpp.entry((module, d.cpp.clone())).or_insert(id);
        r.foreign_path.insert(id, d.cpp.clone());
    }
    // `typedef Texture Texture2D`: a second name for the same symbol, so the two are one
    // type, as they are in C. The typedef names its target by the C++ path it has, which is
    // where namespaces put it rather than where the typedef was written.
    for (name, scope_of, target) in aliases {
        let Some(id) = r.foreign(module, &target) else { continue };
        match namespaces.get(&scope_of) {
            Some(ns) => {
                r.inner.entry((*ns, name)).or_insert(id);
            }
            None => {
                scope.items.entry(name).or_insert(id);
            }
        }
    }
}

/// How a namespace's member is described when a path names it wrongly.
fn member_sort(kind: SymbolKind) -> MemberSort {
    match kind {
        SymbolKind::Fn => MemberSort::Method,
        SymbolKind::Const => MemberSort::Variant,
        _ => MemberSort::AssocType,
    }
}

fn describe_item(item: &Item) -> Option<(&Ident, SymbolKind, Vec<MemberSymbol>)> {
    Some(match &item.kind {
        // Removed by the driver before resolution runs (spec §12.10).
        ItemKind::CfgIf(_) => return None,
        ItemKind::Fn(f) => (&f.name, SymbolKind::Fn, Vec::new()),
        ItemKind::Const(c) => (&c.name, SymbolKind::Const, Vec::new()),
        ItemKind::Static(s) => (&s.name, SymbolKind::Static, Vec::new()),
        ItemKind::Macro(m) => (&m.name, SymbolKind::Macro, Vec::new()),
        ItemKind::Struct(s) => (&s.name, SymbolKind::Struct, struct_members(&s.members)),
        ItemKind::Trait(t) => (&t.name, SymbolKind::Trait, struct_members(&t.items)),
        ItemKind::Union(u) => (
            &u.name,
            SymbolKind::Union,
            u.variants
                .iter()
                .map(|v| MemberSymbol {
                    name: v.name.name.clone(),
                    sort: MemberSort::Variant,
                    // Variants are always as visible as their type (spec §4).
                    vis: Vis::Public,
                    span: v.name.span,
                })
                // A union's methods are named through it too, `State::initial()` (§7.5).
                .chain(struct_members(&u.methods))
                .collect(),
        ),
        ItemKind::Enum(e) => (
            &e.name,
            SymbolKind::Enum,
            e.variants
                .iter()
                .map(|v| MemberSymbol {
                    name: v.name.name.clone(),
                    sort: MemberSort::Variant,
                    vis: Vis::Public,
                    span: v.name.span,
                })
                .collect(),
        ),
        // An `impl` declares nothing nameable: it attaches a trait to a type (spec §7.6).
        ItemKind::Impl(_) | ItemKind::MacroInvocation(_) | ItemKind::Error => return None,
    })
}

fn struct_members(members: &[Member]) -> Vec<MemberSymbol> {
    members
        .iter()
        .filter_map(|m| {
            let (name, sort) = match &m.kind {
                MemberKind::CfgIf(_) => return None,
                MemberKind::Field(f) => (&f.name, MemberSort::Field),
                MemberKind::Fn(f) => (&f.name, MemberSort::Method),
                MemberKind::Ctor(f) => (&f.name, MemberSort::Ctor),
                MemberKind::TypeAlias { name, .. } => (name, MemberSort::AssocType),
                MemberKind::Const(c) => (&c.name, MemberSort::Const),
                MemberKind::Error => return None,
            };
            Some(MemberSymbol { name: name.name.clone(), sort, vis: m.vis, span: name.span })
        })
        .collect()
}

/// Suggest the closest name from `candidates`, for "did you mean" help.
pub fn closest<'a>(target: &str, candidates: impl Iterator<Item = &'a str>) -> Option<String> {
    let mut best: Option<(usize, &str)> = None;
    // Allow roughly one edit per three characters, with a floor for short names.
    let limit = (target.chars().count() / 3).max(1) + 1;
    for c in candidates {
        let d = edit_distance(target, c);
        if d <= limit && best.map_or(true, |(bd, _)| d < bd) {
            best = Some((d, c));
        }
    }
    best.map(|(_, c)| c.to_owned())
}

fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for i in 1..=a.len() {
        cur[0] = i;
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// Is an item with visibility `vis`, declared in `owner`, visible from `viewer`?
pub fn visible_from(vis: Vis, owner: ModuleId, viewer: ModuleId) -> bool {
    if owner == viewer {
        // Within a module, `private` and `pub(module)` are the same thing at top level
        // (spec §4) — there is no file-level privacy.
        true
    } else {
        vis == Vis::Public
    }
}
