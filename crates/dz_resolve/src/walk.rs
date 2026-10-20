//! The resolution walk: imports, then every type and expression in the file.

use std::collections::HashMap;

use dz_ast::*;
use dz_diag::{codes, Diagnostic};
use dz_project::{ModuleId, Workspace};
use dz_span::{FileId, Span};

use crate::{
    closest, visible_from, MemberSort, ParsedFile, Resolution, SymbolId, BUILTIN_FNS, BUILTIN_TYPES,
};

/// What a leading path segment turned out to name.
enum Head {
    Local,
    Builtin,
    Module(ModuleId),
    Symbol(SymbolId),
    /// Already reported; stop looking at the rest of the path.
    Unresolved,
}

/// The name an unaliased `import cpp` goes by: its file name without the extension, so
/// `"Jolt/Physics/Body.h"` is `Body` — when that is an identifier.
fn cpp_alias(header: &str) -> Option<String> {
    let file = header.rsplit(['/', '\\']).next()?;
    let stem = file.split('.').next()?;
    let mut chars = stem.chars();
    let first = chars.next()?;
    let ok =
        (first.is_alphabetic() || first == '_') && chars.all(|c| c.is_alphanumeric() || c == '_');
    ok.then(|| stem.to_owned())
}

/// Pass 1b: record what each `struct X : Base` in this file inherits from.
///
/// A base is a path, so this needs the file's imports in scope — which is why it repeats
/// pass 2's first step. Whatever that reports is reported again, for real, by pass 2;
/// `resolve` discards this round's diagnostics.
pub(crate) fn link_bases(r: &mut Resolution, ws: &Workspace, module: ModuleId, pf: &ParsedFile) {
    if !pf.ast.items.iter().any(|i| matches!(&i.kind, ItemKind::Struct(s) if s.base.is_some())) {
        return;
    }
    let mut w = Walker {
        r: &mut *r,
        ws,
        module,
        file: pf.file,
        generic_spans: Vec::new(),
        module_aliases: HashMap::new(),
        unqualified: HashMap::new(),
        scopes: Vec::new(),
        used_imports: Vec::new(),
        imports: Vec::new(),
        exported: None,
    };
    w.imports(&pf.ast);
    let links: Vec<(SymbolId, SymbolId)> = pf
        .ast
        .items
        .iter()
        .filter_map(|item| {
            let ItemKind::Struct(s) = &item.kind else { return None };
            let base = s.base.as_ref()?;
            // The struct itself, and then the base: both by the same path rules the
            // checker will use, so the two phases cannot disagree about what `: X` names.
            let own = w.r.lookup(module, &s.name.name)?;
            let target = w.struct_by_path(base)?;
            Some((own, target))
        })
        .collect();
    for (own, base) in links {
        // A base that is not a struct, or is the type itself, is `dz_check`'s to report
        // (DZ0541). Recording it here anyway would let `members_with_bases` walk into
        // something that is not a struct, so only a real struct is linked.
        if own != base {
            r.symbols[own.0 as usize].base = Some(base);
        }
    }
}

pub(crate) fn resolve_file(r: &mut Resolution, ws: &Workspace, module: ModuleId, pf: &ParsedFile) {
    let mut w = Walker {
        r: &mut *r,
        ws,
        module,
        file: pf.file,
        generic_spans: Vec::new(),
        module_aliases: HashMap::new(),
        unqualified: HashMap::new(),
        scopes: Vec::new(),
        used_imports: Vec::new(),
        imports: Vec::new(),
        exported: None,
    };
    w.imports(&pf.ast);
    for item in &pf.ast.items {
        w.item(item);
    }
    w.report_unused_imports();

    let record = crate::FileImports {
        module_aliases: w.module_aliases.iter().map(|(k, (m, _))| (k.clone(), *m)).collect(),
        unqualified: w.unqualified.iter().map(|(k, (s, _))| (k.clone(), *s)).collect(),
    };
    r.imports.insert(pf.file, record);
}

struct Walker<'a, 'w> {
    r: &'a mut Resolution,
    ws: &'w Workspace,
    module: ModuleId,
    file: FileId,
    /// Spans of generic-parameter bindings, so a binding found in scope can be told apart
    /// from a local value of the same name.
    generic_spans: Vec<Span>,
    /// `import engine::render as gfx` → "gfx" → that module.
    module_aliases: HashMap<String, (ModuleId, usize)>,
    /// `import math::{ pow }` → "pow" → that symbol.
    unqualified: HashMap<String, (SymbolId, usize)>,
    /// Innermost scope last. Each holds local binding names.
    scopes: Vec<HashMap<String, Span>>,
    used_imports: Vec<bool>,
    imports: Vec<(String, Span)>,
    /// Set while walking the signature of a declaration other modules can use, to its name:
    /// a type named there must be one they can name too (spec §3.2, DZ0311).
    exported: Option<String>,
}

impl<'a, 'w> Walker<'a, 'w> {
    fn error(&mut self, d: Diagnostic) {
        self.r.diagnostics.push(d);
    }

    // --- imports ----------------------------------------------------------

    /// A file's own imports: `import m::{ A, B }` and `import m as x`, the exceptions to
    /// what the manifest names for every file (spec §3.2).
    fn imports(&mut self, ast: &SourceFileAst) {
        for import in &ast.imports {
            let index = self.imports.len();
            match &import.kind {
                ImportKind::Module { path, alias: Some(alias) } => {
                    let Some(target) = self.find_module(path) else { continue };
                    self.use_key(&path.segments[0].name.name);
                    self.imports.push((alias.name.clone(), import.span));
                    self.used_imports.push(false);
                    self.module_aliases.insert(alias.name.clone(), (target, index));
                }
                ImportKind::Module { path, alias: None } => {
                    let text = path_text(path);
                    let file = self.file;
                    let mut d = Diagnostic::error(
                        codes::IMPORT_FORM_REMOVED,
                        format!("`import {text}` names nothing new"),
                    )
                    .at(file, import.span, "not an import form");
                    d = if self.find_module_quiet(path).is_some() {
                        d.with_help(format!(
                            "`{text}` is already reachable in every file of this module, through \
                             dazzle.toml; remove this line, or write `import {text}::{{ Name }}` \
                             or `import {text} as name` (spec §3.2)"
                        ))
                    } else {
                        d.with_help(format!(
                            "a module's dependencies are listed in its dazzle.toml: \
                             `{} = {{ path = \"…\" }}` under `[dependencies]` (spec §3.2)",
                            last_segment(&text)
                        ))
                    };
                    self.error(d);
                }
                ImportKind::Selective { path, names } => {
                    let Some(target) = self.find_module(path) else { continue };
                    self.use_key(&path.segments[0].name.name);
                    for n in names {
                        let Some(sym) = self.r.lookup(target, &n.name) else {
                            self.unknown_member_of_module(target, &n.name, n.span);
                            continue;
                        };
                        if !self.check_item_visible(sym, n.span) {
                            continue;
                        }
                        let index = self.imports.len();
                        self.imports.push((n.name.clone(), n.span));
                        self.used_imports.push(false);
                        if let Some((prev, _)) = self.unqualified.get(&n.name) {
                            let prev_name = self.r.symbol(*prev).name.clone();
                            let file = self.file;
                            self.error(
                                Diagnostic::error(
                                    codes::AMBIGUOUS_IMPORT,
                                    format!("`{}` is imported more than once", n.name),
                                )
                                .at(
                                    file,
                                    n.span,
                                    format!("`{prev_name}` is already in scope"),
                                ),
                            );
                            continue;
                        }
                        self.unqualified.insert(n.name.clone(), (sym, index));
                    }
                }
                // A header is named once, in its recipe (spec §15.1).
                ImportKind::Cpp { name, alias, .. } => {
                    let key = alias
                        .as_ref()
                        .map(|a| a.name.clone())
                        .or_else(|| cpp_alias(name))
                        .unwrap_or_else(|| "lib".to_owned());
                    let recipe = cpp_alias(name).unwrap_or_else(|| key.clone());
                    let file = self.file;
                    self.error(
                        Diagnostic::error(
                            codes::IMPORT_FORM_REMOVED,
                            "a C or C++ header is imported in dazzle.toml, not in a source file",
                        )
                        .at(file, import.span, "`import cpp` is no longer the language")
                        .with_help(format!(
                            "name the header in its recipe, `import = [\"{name}\"]` under \
                             `[cpp.libs.{recipe}]`, and depend on it: `{key} = {{ cpp = \
                             \"{recipe}\" }}` under `[dependencies]` (spec §15.1)"
                        )),
                    );
                }
                ImportKind::Error => {}
            }
        }
    }

    /// The module an import's path names: a dependency key of this module (or an alias an
    /// earlier line of this file gave), then each further segment a public dependency of the
    /// one before (spec §3.2). Reported when it names nothing.
    fn find_module(&mut self, path: &Path) -> Option<ModuleId> {
        if let Some(m) = self.find_module_quiet(path) {
            return Some(m);
        }
        let names: Vec<&str> = path.segments.iter().map(|s| s.name.name.as_str()).collect();
        let text = names.join("::");
        let file = self.file;
        // Where the walk stopped, to say which segment is wrong.
        let head = names[0];
        let known: Vec<String> = self.dep_keys();
        let Some(mut at) = self.module_named(head) else {
            let mut d =
                Diagnostic::error(codes::UNKNOWN_MODULE, format!("no module `{head}` in scope"))
                    .at(file, path.segments[0].name.span, "not a dependency of this module");
            d = match closest(head, known.iter().map(String::as_str)) {
                Some(s) => d.with_help(format!("did you mean `{s}`?")),
                None if head == dz_project::STD_MODULE => d.with_help(
                    "list the standard library in dazzle.toml: `std = { open = true }` under \
                     `[dependencies]` (spec §14.4)",
                ),
                None => d.with_help(format!(
                    "add it to `[dependencies]` in this module's dazzle.toml, as `{head} = {{ path \
                     = \"…\" }}` (spec §3.2)"
                )),
            };
            self.error(d);
            return None;
        };
        let mut owner = head.to_owned();
        for seg in &path.segments[1..] {
            let public = self.r.names.get(&at).map(|n| n.public.clone()).unwrap_or_default();
            match public.get(&seg.name.name) {
                Some(next) => {
                    at = *next;
                    owner = seg.name.name.clone();
                }
                None => {
                    self.error(
                        Diagnostic::error(
                            codes::UNKNOWN_MODULE,
                            format!("no module `{text}` in scope"),
                        )
                        .at(file, seg.name.span, format!("not a public dependency of `{owner}`"))
                        .with_help(format!(
                            "a path reaches only the dependencies `{owner}` marks `pub = true` \
                             (spec §3.2)"
                        )),
                    );
                    return None;
                }
            }
        }
        None
    }

    /// Record that this module went through the dependency key `key`, when it is one — and
    /// not an alias an import line of this file made (spec §3.2).
    fn use_key(&mut self, key: &str) {
        if self.module_aliases.contains_key(key) {
            return;
        }
        if self.r.names.get(&self.module).is_some_and(|n| n.deps.contains_key(key)) {
            self.r.used_deps.insert((self.module, key.to_owned()));
        }
    }

    /// A bare name found in an opened dependency uses every key that opens its module.
    fn use_opened(&mut self, sym: SymbolId) {
        let owner = self.r.symbol(sym).module;
        let keys: Vec<String> = self
            .r
            .names
            .get(&self.module)
            .map(|n| n.deps.iter().filter(|(_, m)| **m == owner).map(|(k, _)| k.clone()).collect())
            .unwrap_or_default();
        for key in keys {
            self.r.used_deps.insert((self.module, key));
        }
    }

    /// [`Walker::find_module`] without the report.
    fn find_module_quiet(&self, path: &Path) -> Option<ModuleId> {
        let first = path.segments.first()?;
        let mut at = self.module_named(&first.name.name)?;
        for seg in &path.segments[1..] {
            at = *self.r.names.get(&at)?.public.get(&seg.name.name)?;
        }
        Some(at)
    }

    /// A module by the name this file writes it with: an alias from an earlier import line,
    /// or a dependency key.
    fn module_named(&self, name: &str) -> Option<ModuleId> {
        if let Some((m, _)) = self.module_aliases.get(name) {
            return Some(*m);
        }
        self.r.names.get(&self.module)?.deps.get(name).copied()
    }

    fn dep_keys(&self) -> Vec<String> {
        let mut keys: Vec<String> = self
            .r
            .names
            .get(&self.module)
            .map(|n| n.deps.keys().cloned().collect())
            .unwrap_or_default();
        keys.extend(self.module_aliases.keys().cloned());
        keys.sort();
        keys
    }

    fn report_unused_imports(&mut self) {
        for (i, used) in self.used_imports.clone().iter().enumerate() {
            if *used {
                continue;
            }
            let (name, span) = self.imports[i].clone();
            let file = self.file;
            self.error(
                Diagnostic::warning(codes::UNUSED_IMPORT, format!("unused import `{name}`"))
                    .at(file, span, "never referred to in this file")
                    .with_help("imports are file-scoped (spec §3.2); remove it or use it"),
            );
        }
    }

    // --- scopes -----------------------------------------------------------

    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    /// Introduce a binding, rejecting a duplicate in the *same* scope (spec §7.1).
    fn bind(&mut self, name: &str, span: Span) {
        if name == "_" {
            return;
        }
        if let Some(scope) = self.scopes.last() {
            if let Some(prev) = scope.get(name).copied() {
                let file = self.file;
                self.error(
                    Diagnostic::error(
                        codes::SHADOWED_BINDING,
                        format!("`{name}` is already bound in this scope"),
                    )
                    .at(file, span, "redeclared here")
                    .with_label(dz_diag::Label::secondary(file, prev, "first bound here"))
                    .with_help(
                        "shadowing is allowed in an inner scope but not in the same one \
                         (spec §7.1)",
                    ),
                );
                return;
            }
        }
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.to_owned(), span);
        }
    }

    fn bind_pattern(&mut self, p: &Pat) {
        match &p.kind {
            PatKind::Ident(i) => self.bind(&i.name, i.span),
            PatKind::Tuple(elems) => {
                for e in elems {
                    self.bind_pattern(e);
                }
            }
            PatKind::Wild | PatKind::Error => {}
        }
    }

    fn is_local(&self, name: &str) -> bool {
        self.scopes.iter().rev().any(|s| s.contains_key(name))
    }

    // --- items ------------------------------------------------------------

    fn item(&mut self, item: &Item) {
        // At module level when no scope is open: a declaration inside a body has one.
        let target = match &item.kind {
            ItemKind::Fn(_) if self.scopes.is_empty() => AttrTarget::ModuleFn,
            ItemKind::Fn(_) => AttrTarget::Other("a function inside a body"),
            ItemKind::Static(_) => AttrTarget::Static,
            ItemKind::Struct(_) => AttrTarget::Other("a struct"),
            ItemKind::Union(_) => AttrTarget::Other("a union"),
            ItemKind::Enum(_) => AttrTarget::Other("an enum"),
            ItemKind::Trait(_) => AttrTarget::Other("a trait"),
            ItemKind::Impl(_) => AttrTarget::Other("an implementation"),
            ItemKind::Const(_) => AttrTarget::Other("a constant"),
            _ => AttrTarget::Other("this declaration"),
        };
        self.attributes(&item.attrs, target);
        match &item.kind {
            // Removed by the driver before resolution runs (spec §12.10).
            ItemKind::CfgIf(_) => {}
            ItemKind::Fn(f) => {
                // How the standard library reaches the operating system: the body is the
                // compiler's C++ (spec §7.2, 18.65). A program's own way out is `import cpp`.
                if f.is_extern && (self.module != self.ws.prelude || !self.scopes.is_empty()) {
                    let file = self.file;
                    let help = if self.module != self.ws.prelude {
                        "to call C or C++, name the library in `dazzle.toml` (spec §15.1)"
                    } else {
                        "an `extern fn` is declared at module level"
                    };
                    self.error(
                        Diagnostic::error(
                            codes::EXTERN_OUTSIDE_STD,
                            format!("`{}` cannot be `extern`", f.name.name),
                        )
                        .at(file, f.name.span, "`extern fn` is for the standard library")
                        .with_help(help),
                    );
                }
                self.exported = (item.vis == Vis::Public).then(|| f.name.name.clone());
                self.function(f, None)
            }
            ItemKind::Const(c) => {
                if let Some(t) = &c.ty {
                    self.exported = (item.vis == Vis::Public).then(|| c.name.name.clone());
                    self.ty(t);
                    self.exported = None;
                }
                self.push_scope();
                self.expr(&c.value);
                self.pop_scope();
            }
            ItemKind::Static(s) => {
                self.exported = (item.vis == Vis::Public).then(|| s.name.name.clone());
                self.ty(&s.ty);
                self.exported = None;
                self.push_scope();
                self.expr(&s.value);
                self.pop_scope();
            }
            ItemKind::Struct(s) => {
                // A type's generic parameters are in scope for its own members.
                self.push_scope();
                self.bind_generics(&s.generics);
                self.bind_assoc_types(&s.members);
                if let Some(b) = &s.base {
                    self.path(b, PathUse::Type);
                }
                let mut fields: Vec<String> = s
                    .members
                    .iter()
                    .filter_map(|m| match &m.kind {
                        MemberKind::Field(f) => Some(f.name.name.clone()),
                        _ => None,
                    })
                    .collect();
                // A base's fields are in scope in the derived type's methods exactly as
                // its own are (spec §7.3). Visibility is the checker's to enforce; the
                // resolver's job is only that the name is not "not found".
                if let Some(sym) = self.r.lookup(self.module, &s.name.name) {
                    if let Some(base) = self.r.symbol(sym).base {
                        for m in self.r.members_with_bases(base) {
                            if m.sort == crate::MemberSort::Field && !fields.contains(&m.name) {
                                fields.push(m.name.clone());
                            }
                        }
                    }
                }
                let owner = (item.vis == Vis::Public).then_some(&s.name.name);
                self.members(&s.members, Some(&fields), owner);
                self.pop_scope();
            }
            ItemKind::Trait(t) => {
                self.push_scope();
                self.bind_generics(&t.generics);
                self.bind_assoc_types(&t.items);
                for b in &t.supertraits {
                    self.path(b, PathUse::Type);
                }
                // A trait has no fields, but its default methods do have a `self` (spec
                // §7.6) — the same scope that binds a struct's fields binds that.
                let owner = (item.vis == Vis::Public).then_some(&t.name.name);
                self.members(&t.items, Some(&[]), owner);
                self.pop_scope();
            }
            ItemKind::Impl(i) => {
                self.push_scope();
                self.bind_generics(&i.generics);
                self.bind_assoc_types(&i.items);
                if let Some(p) = &i.trait_path {
                    self.path(p, PathUse::Type);
                }
                self.ty(&i.self_ty);
                // An implementation's methods see the type's fields unqualified, as the
                // type's own do (spec §7.3) — all of them in the type's own module, and
                // only the `pub` ones from anywhere else (spec §4, §7.6).
                let fields = self.fields_of_type(&i.self_ty);
                self.members(&i.items, Some(&fields), None);
                self.pop_scope();
            }
            ItemKind::Union(u) => {
                self.push_scope();
                self.bind_generics(&u.generics);
                for v in &u.variants {
                    for f in &v.fields {
                        self.field(f);
                    }
                }
                // A union's methods have a `self` and no fields to see bare (spec §7.5).
                let owner = (item.vis == Vis::Public).then_some(&u.name.name);
                self.members(&u.methods, Some(&[]), owner);
                self.pop_scope();
            }
            ItemKind::Enum(e) => {
                if let Some(b) = &e.backing {
                    self.ty(b);
                }
                for v in &e.variants {
                    if let Some(val) = &v.value {
                        self.push_scope();
                        self.expr(val);
                        self.pop_scope();
                    }
                }
            }
            // A macro's body is tokens, and an invocation is expanded before resolution
            // (spec §12.11).
            ItemKind::Macro(_) | ItemKind::MacroInvocation(_) | ItemKind::Error => {}
        }
    }

    /// The struct an `impl`'s type names, if it names one. Resolution has already reported
    /// a name that does not resolve; this only looks.
    fn struct_of(&self, t: &Type) -> Option<SymbolId> {
        let TypeKind::Path(p) = &t.kind else { return None };
        self.struct_by_path(p)
    }

    /// The struct a type path names, by the same rules `struct_of` uses — the base of a
    /// `struct X : Base` is a bare path rather than a `Type` (spec §7.3).
    fn struct_by_path(&self, p: &Path) -> Option<SymbolId> {
        let sym = match p.segments.as_slice() {
            [one] => {
                let name = &one.name.name;
                self.r
                    .lookup(self.module, name)
                    .or_else(|| self.unqualified.get(name).map(|(s, _)| *s))
                    .or_else(|| {
                        self.r.names.get(&self.module).and_then(|n| n.picks.get(name)).copied()
                    })
                    .or_else(|| {
                        match self.r.find_opened(self.module, &self.opened_modules(), name) {
                            crate::Opened::One(s) => Some(s),
                            _ => None,
                        }
                    })?
            }
            [first, ..] => {
                let mut at = self.module_named(&first.name.name)?;
                let (last, middle) = p.segments[1..].split_last()?;
                for seg in middle {
                    at = *self.r.names.get(&at)?.public.get(&seg.name.name)?;
                }
                self.r.lookup(at, &last.name.name)?
            }
            [] => return None,
        };
        (self.r.symbol(sym).kind == crate::SymbolKind::Struct).then_some(sym)
    }

    fn fields_of_type(&self, t: &Type) -> Vec<String> {
        let Some(sym) = self.struct_of(t) else { return Vec::new() };
        let s = self.r.symbol(sym);
        s.members
            .iter()
            .filter(|m| m.sort == crate::MemberSort::Field)
            .filter(|m| s.module == self.module || m.vis == Vis::Public)
            .map(|m| m.name.clone())
            .collect()
    }

    /// A type's members. `owner` is the type's name when other modules can use it, so the
    /// signatures of its public members are held to what they can name (spec §3.2).
    fn members(
        &mut self,
        members: &[Member],
        fields_in_scope: Option<&[String]>,
        owner: Option<&String>,
    ) {
        // A trait's items are as public as the trait (spec §4); a struct's are their own.
        let is_trait = fields_in_scope.is_some_and(|f| f.is_empty());
        for m in members {
            let what = match &m.kind {
                MemberKind::Field(_) => "a field",
                MemberKind::Fn(_) => "a method",
                MemberKind::Ctor(_) => "a constructor",
                MemberKind::Const(_) => "an associated constant",
                _ => "a member",
            };
            self.attributes(&m.attrs, AttrTarget::Other(what));
            let public = owner.is_some() && (is_trait || m.vis == Vis::Public);
            let exported = |name: &str| {
                public.then(|| format!("{}::{name}", owner.map(String::as_str).unwrap_or("")))
            };
            match &m.kind {
                MemberKind::CfgIf(_) => {}
                MemberKind::Field(f) => {
                    self.exported = exported(&f.name.name);
                    self.field(f)
                }
                // A function with no receiver has no `self` whose fields it could see; a
                // constructor is building one, so it does (spec §7.2, §7.3).
                MemberKind::Fn(f) if f.self_param.is_none() => {
                    self.exported = exported(&f.name.name);
                    self.function(f, None)
                }
                MemberKind::Fn(f) | MemberKind::Ctor(f) => {
                    self.exported = exported(&f.name.name);
                    self.function(f, fields_in_scope)
                }
                MemberKind::TypeAlias { ty, .. } => {
                    if let Some(t) = ty {
                        self.ty(t);
                    }
                }
                // Like a module's `const`: no `self`, so the type's fields are not in scope.
                MemberKind::Const(c) => {
                    if let Some(t) = &c.ty {
                        self.ty(t);
                    }
                    self.push_scope();
                    self.expr(&c.value);
                    self.pop_scope();
                }
                MemberKind::Error => {}
            }
        }
    }

    /// Bring a declaration's generic parameters into scope as type names.
    fn bind_generics(&mut self, g: &Generics) {
        for p in &g.params {
            self.bind(&p.name.name, p.name.span);
            // Remembered so `T::is_trivially_copyable` is recognised as a type query
            // rather than a path into a module named `T` (spec §5.5).
            self.generic_spans.push(p.name.span);
            for b in &p.bounds {
                self.path(b, PathUse::Type);
            }
            if let Some(d) = &p.default {
                self.ty(d);
            }
        }
    }

    /// Is `name` a generic parameter in scope, as opposed to a local value?
    fn is_type_param(&self, name: &str) -> bool {
        self.scopes
            .iter()
            .rev()
            .find_map(|s| s.get(name))
            .is_some_and(|span| self.generic_spans.contains(span))
    }

    /// An associated type is nameable inside the declaration that has it, whether that
    /// declaration is the trait naming it or the implementer choosing it (spec §8.5).
    fn bind_assoc_types(&mut self, members: &[Member]) {
        for m in members {
            if let MemberKind::TypeAlias { name, .. } = &m.kind {
                self.bind(&name.name, name.span);
            }
        }
    }

    fn field(&mut self, f: &Field) {
        self.ty(&f.ty);
        self.exported = None;
        if let Some(d) = &f.default {
            self.push_scope();
            self.expr(d);
            self.pop_scope();
        }
    }

    fn function(&mut self, f: &FnDecl, fields_in_scope: Option<&[String]>) {
        self.push_scope();

        self.bind_generics(&f.generics);

        // Inside a method, the struct's fields are in scope unqualified (spec §7.3).
        // They go in their own scope so a parameter of the same name shadows the field
        // rather than colliding with it.
        if let Some(fields) = fields_in_scope {
            self.push_scope();
            if let Some(scope) = self.scopes.last_mut() {
                scope.insert("self".to_owned(), Span::DUMMY);
                for field in fields {
                    scope.entry(field.clone()).or_insert(Span::DUMMY);
                }
            }
        }
        self.push_scope();

        // The signature, and only the signature, is what another module sees.
        let exported = self.exported.take();
        for p in &f.params {
            self.exported = exported.clone();
            self.ty(&p.ty);
            self.exported = None;
            if let Some(d) = &p.default {
                self.expr(d);
            }
            self.bind(&p.name.name, p.name.span);
        }
        if let Some(ret) = &f.ret {
            self.exported = exported;
            self.ty(ret);
            self.exported = None;
        }
        if let Some(body) = &f.body {
            self.block(body);
        }
        self.pop_scope(); // parameters
        if fields_in_scope.is_some() {
            self.pop_scope(); // fields
        }
        self.pop_scope(); // generics
    }

    // --- types ------------------------------------------------------------

    fn ty(&mut self, t: &Type) {
        match &t.kind {
            TypeKind::Path(p) => self.path(p, PathUse::Type),
            TypeKind::Optional(i) => self.ty(i),
            TypeKind::Ptr { inner, .. }
            | TypeKind::OwnPtr { inner, .. }
            | TypeKind::Ref { inner, .. } => self.ty(inner),
            TypeKind::Array { size, elem } => {
                self.expr(size);
                self.ty(elem);
            }
            TypeKind::Slice { elem, .. } => self.ty(elem),
            TypeKind::Tuple(elems) => {
                for e in elems {
                    self.ty(e);
                }
            }
            TypeKind::Fn { params, ret } => {
                for p in params {
                    self.ty(p);
                }
                if let Some(r) = ret {
                    self.ty(r);
                }
            }
            TypeKind::SelfType | TypeKind::Error => {}
        }
    }

    // --- statements and expressions ---------------------------------------

    fn block(&mut self, b: &Block) {
        self.push_scope();
        for s in &b.stmts {
            self.stmt(s);
        }
        self.pop_scope();
    }

    fn stmt(&mut self, s: &Stmt) {
        match &s.kind {
            // Expanded or stripped before resolution (spec §12.5, §12.10).
            StmtKind::CfgIf(_) | StmtKind::Macro(_) => {}
            StmtKind::Let(l) => {
                if let Some(t) = &l.ty {
                    self.ty(t);
                }
                // The initialiser is resolved before the binding exists, so
                // `let x = x` refers to an outer `x` rather than to itself.
                if let Some(init) = &l.init {
                    self.expr(init);
                }
                self.bind_pattern(&l.pat);
            }
            StmtKind::Assign { target, value, .. } => {
                self.expr(target);
                self.expr(value);
            }
            StmtKind::Expr(e) => self.expr(e),
            StmtKind::While(w) => {
                self.expr(&w.cond);
                self.block(&w.body);
            }
            StmtKind::For(f) => {
                self.expr(&f.iter);
                self.push_scope();
                for b in &f.bindings {
                    self.bind_pattern(&b.pat);
                }
                self.block(&f.body);
                self.pop_scope();
            }
            StmtKind::Defer(inner) => self.stmt(inner),
            StmtKind::Item(item) => self.item(item),
            StmtKind::Error => {}
        }
    }

    fn expr(&mut self, e: &Expr) {
        match &e.kind {
            ExprKind::Path(p) => self.path(p, PathUse::Value),
            ExprKind::StructLit { path, fields } => {
                self.path(path, PathUse::Type);
                for f in fields {
                    self.expr(&f.value);
                }
            }
            // `.Transition{}` needs the expected type, so it waits for M3.
            ExprKind::DotInfer { init, .. } => {
                if let Some(fields) = init {
                    for f in fields {
                        self.expr(&f.value);
                    }
                }
            }
            ExprKind::ArrayLit(elems) => {
                for x in elems {
                    self.expr(x);
                }
            }
            ExprKind::Call { callee, args } => {
                self.expr(callee);
                for a in args {
                    self.expr(a);
                }
            }
            ExprKind::Index { base, index } => {
                self.expr(base);
                self.expr(index);
            }
            // The field name needs the receiver's type (M3); the receiver resolves now.
            ExprKind::Field { base, generics, .. } => {
                self.expr(base);
                // `xs.map<Vec2>(f)` names `Vec2` (spec §8.1).
                for t in generics {
                    self.ty(t);
                }
            }
            ExprKind::Unary { operand, .. } => self.expr(operand),
            ExprKind::Binary { lhs, rhs, .. } => {
                self.expr(lhs);
                self.expr(rhs);
            }
            ExprKind::Cast { expr, ty } => {
                self.expr(expr);
                self.ty(ty);
            }
            ExprKind::Range { start, end, .. } => {
                if let Some(s) = start {
                    self.expr(s);
                }
                if let Some(s) = end {
                    self.expr(s);
                }
            }
            ExprKind::Ref { expr, .. }
            | ExprKind::Deref(expr)
            | ExprKind::Move(expr)
            | ExprKind::Named { value: expr, .. } => self.expr(expr),
            ExprKind::Or { expr, clause } => {
                self.expr(expr);
                match clause.as_ref() {
                    OrClause::Value(v) => self.expr(v),
                    OrClause::Block { binding, body } => {
                        self.push_scope();
                        // The error binding is named explicitly or not at all — there is
                        // no implicit `it` in an `or` clause (spec §9.4).
                        if let Some(b) = binding {
                            self.bind(&b.name, b.span);
                        }
                        self.block(body);
                        self.pop_scope();
                    }
                    OrClause::Ignore(_) | OrClause::Unreachable(_) => {}
                }
            }
            ExprKind::Block(b) | ExprKind::Unsafe(b) => self.block(b),
            ExprKind::If(i) => {
                self.expr(&i.cond);
                // `if let x = v`: `x` exists in the `then` block and nowhere else.
                match &i.binding {
                    Some(b) => {
                        self.push_scope();
                        self.bind(&b.name.name, b.name.span);
                        self.block(&i.then);
                        self.pop_scope();
                    }
                    None => self.block(&i.then),
                }
                if let Some(e) = &i.else_branch {
                    self.expr(e);
                }
            }
            ExprKind::Switch(s) => {
                self.expr(&s.subject);
                for arm in &s.arms {
                    self.push_scope();
                    for p in &arm.patterns {
                        match p {
                            CasePat::Variant(v) => {
                                if let Some(b) = &v.binding {
                                    self.bind(&b.name, b.span);
                                }
                            }
                            // `case 1`, `case lib::MAX` — an ordinary expression. A bare
                            // name is a `Variant` whatever it turns out to be, and the
                            // checker resolves it once it knows the subject's type.
                            CasePat::Value(e) => self.expr(e),
                        }
                    }
                    self.block(&arm.body);
                    self.pop_scope();
                }
                if let Some(d) = &s.default {
                    self.block(d);
                }
            }
            ExprKind::Return(v) => {
                if let Some(v) = v {
                    self.expr(v);
                }
            }
            ExprKind::Break { .. } => {}
            ExprKind::Intrinsic { args, .. } => {
                for a in args {
                    self.expr(a);
                }
            }
            ExprKind::Tuple(elems) => {
                for e in elems {
                    self.expr(e);
                }
            }
            ExprKind::Int(_)
            | ExprKind::Float(_)
            | ExprKind::Str(_)
            | ExprKind::RawStr(_)
            | ExprKind::Char(_)
            | ExprKind::Bool(_)
            | ExprKind::Null
            | ExprKind::None
            | ExprKind::Continue { .. }
            | ExprKind::MacroCall(_)
            | ExprKind::Error => {}
        }
    }

    // --- paths ------------------------------------------------------------

    fn path(&mut self, p: &Path, use_kind: PathUse) {
        for seg in &p.segments {
            for g in &seg.generics {
                self.ty(g);
            }
        }
        let Some(first) = p.segments.first() else { return };

        let head = self.resolve_head(&first.name, use_kind, p.segments.len() > 1);
        match head {
            Head::Local | Head::Builtin | Head::Unresolved => {}
            Head::Module(mut target) => {
                // Further modules first: `rl::shapes::Rectangle` walks the public dependency
                // `shapes` of `rl` (spec §3.2). A declaration of the same name wins.
                let mut at = 1;
                while let Some(seg) = p.segments.get(at) {
                    if self.r.lookup(target, &seg.name.name).is_some() {
                        break;
                    }
                    match self.r.names.get(&target).and_then(|n| n.public.get(&seg.name.name)) {
                        Some(next) => {
                            target = *next;
                            at += 1;
                        }
                        None => break,
                    }
                }
                let Some(second) = p.segments.get(at) else {
                    // A bare module name is not a value; M3 reports the type error.
                    return;
                };
                let Some(sym) = self.r.lookup(target, &second.name.name) else {
                    self.unknown_member_of_module(target, &second.name.name, second.name.span);
                    return;
                };
                if !self.check_item_visible(sym, second.name.span) {
                    return;
                }
                if use_kind == PathUse::Type {
                    self.check_exported(sym, p.span);
                }
                self.check_members(sym, &p.segments[at + 1..]);
            }
            Head::Symbol(sym) => {
                if use_kind == PathUse::Type {
                    self.check_exported(sym, p.span);
                }
                self.check_members(sym, &p.segments[1..])
            }
        }
    }

    /// Every attribute on a declaration must be one the language has, on a declaration it
    /// applies to, without arguments it does not take, and once (spec §12.8, 18.60).
    fn attributes(&mut self, attrs: &[Attr], target: AttrTarget) {
        /// Described by the specification, not implemented by the compiler: each with where.
        const LATER: &[(&str, &str)] = &[
            ("inline", "§12.8"),
            ("align", "§12.8"),
            ("packed", "§12.8"),
            ("deprecated", "§12.8"),
            ("export", "§15.5"),
            ("derive", "§12"),
            ("persist", "§17.6"),
            ("raw", "§7.5"),
        ];
        const KNOWN: &[&str] = &["test", "bench", "thread_local"];
        let file = self.file;
        for (i, a) in attrs.iter().enumerate() {
            let name = a.name.name.as_str();
            if let Some((_, section)) = LATER.iter().find(|(n, _)| *n == name) {
                self.error(
                    Diagnostic::error(
                        codes::ATTRIBUTE_NOT_IMPLEMENTED,
                        format!("`#[{name}]` is not implemented yet"),
                    )
                    .at(file, a.span, "the compiler does not do what this asks")
                    .with_help(format!(
                        "spec {section} describes it, but the compiler does not build it yet; \
                         remove it rather than rely on it"
                    )),
                );
                continue;
            }
            if !KNOWN.contains(&name) {
                let mut d = Diagnostic::error(
                    codes::UNKNOWN_ATTRIBUTE,
                    format!("`#[{name}]` is not an attribute"),
                )
                .at(file, a.span, "unknown attribute");
                d = match (name, closest(name, KNOWN.iter().copied())) {
                    ("cfg", _) => {
                        d.with_help("conditional compilation is `#if FLAG { … }` (spec §12.10)")
                    }
                    (_, Some(s)) => d.with_help(format!("did you mean `#[{s}]`?")),
                    (_, None) => d.with_help(
                        "the attributes are `#[test]`, `#[bench]` and `#[thread_local]` \
                         (spec §12.8)",
                    ),
                };
                self.error(d);
                continue;
            }
            if attrs[..i].iter().any(|b| b.name.name == name) {
                self.error(
                    Diagnostic::error(
                        codes::MISPLACED_ATTRIBUTE,
                        format!("`#[{name}]` is written twice"),
                    )
                    .at(file, a.span, "again")
                    .with_help("once says it"),
                );
                continue;
            }
            let (fits, applies_to) = match name {
                "test" | "bench" => {
                    (matches!(target, AttrTarget::ModuleFn), "a function at module level")
                }
                _ => (matches!(target, AttrTarget::Static), "a `static`"),
            };
            if !fits {
                let on = match target {
                    AttrTarget::ModuleFn => "a function",
                    AttrTarget::Static => "a static",
                    AttrTarget::Other(what) => what,
                };
                self.error(
                    Diagnostic::error(
                        codes::MISPLACED_ATTRIBUTE,
                        format!("`#[{name}]` does not apply to {on}"),
                    )
                    .at(file, a.span, format!("`#[{name}]` goes on {applies_to}"))
                    .with_help("see the attributes and what they apply to in spec §12.8"),
                );
                continue;
            }
            if !a.args.is_empty() {
                self.error(
                    Diagnostic::error(
                        codes::MISPLACED_ATTRIBUTE,
                        format!("`#[{name}]` takes no arguments"),
                    )
                    .at(file, a.span, "unexpected arguments")
                    .with_help(format!("write `#[{name}]` on its own")),
                );
            }
        }
    }

    /// A type in the signature of something other modules can use must be one they can
    /// name: this module's own, the standard library's, or a public dependency's
    /// (spec §3.2, DZ0311).
    fn check_exported(&mut self, sym: SymbolId, span: Span) {
        let Some(decl) = self.exported.clone() else { return };
        // An executable has no dependents, so its `pub` is no interface to keep whole.
        if self.ws.module(self.module).build.output == dz_project::OutputKind::Executable {
            return;
        }
        let owner = self.r.symbol(sym).module;
        if owner == self.module || owner == self.ws.prelude {
            return;
        }
        // Everything a dependent can reach: the public dependencies, and theirs.
        let mut reachable: Vec<ModuleId> = Vec::new();
        let mut stack = vec![self.module];
        while let Some(m) = stack.pop() {
            let public: Vec<ModuleId> = self
                .r
                .names
                .get(&m)
                .map(|n| n.public.values().copied().collect())
                .unwrap_or_default();
            for next in public {
                if !reachable.contains(&next) {
                    reachable.push(next);
                    stack.push(next);
                }
            }
        }
        if reachable.contains(&owner) {
            return;
        }
        let key = self
            .ws
            .module(self.module)
            .deps
            .iter()
            .find(|d| d.module == owner)
            .map(|d| d.alias.clone())
            .unwrap_or_else(|| self.ws.module(owner).name.clone());
        let ty = self.r.symbol(sym).name.clone();
        let file = self.file;
        self.error(
            Diagnostic::error(
                codes::PRIVATE_DEPENDENCY_IN_PUBLIC,
                format!("`{decl}` is public, but `{ty}` comes from the private dependency `{key}`"),
            )
            .at(file, span, "modules that use this one cannot name this type")
            .with_help(format!(
                "mark `{key}` `pub = true` in dazzle.toml to make it part of this module's \
                 interface, or use a type of the module's own here (spec §3.2)"
            )),
        );
    }

    fn opened_modules(&self) -> Vec<ModuleId> {
        self.r.names.get(&self.module).map(|n| n.opened.clone()).unwrap_or_default()
    }

    /// A bare name two opened dependencies both export. Reported where it is used rather
    /// than in the manifest, so a library adding a name breaks only code that writes that
    /// name (spec §3.2).
    fn ambiguous_opened(&mut self, name: &Ident, found: &[SymbolId]) {
        // Each by the key this module reaches it under, so the help is code that works.
        let keys: Vec<String> = found
            .iter()
            .map(|s| {
                let m = self.r.symbol(*s).module;
                self.r
                    .module_spelling(self.file, m)
                    .unwrap_or_else(|| self.ws.module(m).name.clone())
            })
            .collect();
        let paths: Vec<String> = keys.iter().map(|k| format!("`{k}::{}`", name.name)).collect();
        let quoted: Vec<String> = keys.iter().map(|k| format!("`{k}`")).collect();
        let file = self.file;
        self.error(
            Diagnostic::error(
                codes::AMBIGUOUS_NAME,
                format!("`{}` is ambiguous: more than one opened dependency exports it", name.name),
            )
            .at(file, name.span, format!("could be from {}", quoted.join(" or ")))
            .with_help(format!(
                "write {}, or choose one for the module in dazzle.toml with `[names]` \
                 `{} = \"{}\"` (spec §3.2)",
                paths.join(" or "),
                name.name,
                keys[0]
            )),
        );
    }

    fn resolve_head(&mut self, name: &Ident, use_kind: PathUse, qualified: bool) -> Head {
        if !qualified && self.is_local(&name.name) {
            return Head::Local;
        }
        if let Some((m, idx)) = self.module_aliases.get(&name.name).copied() {
            self.used_imports[idx] = true;
            return Head::Module(m);
        }
        // A dependency key from the manifest (spec §3.2).
        if let Some(m) =
            self.r.names.get(&self.module).and_then(|n| n.deps.get(&name.name)).copied()
        {
            self.use_key(&name.name);
            return Head::Module(m);
        }
        // `T::is_trivially_copyable` — a compile-time query on a generic parameter
        // (spec §5.5). Which queries exist is the checker's business; resolution only has
        // to not mistake `T` for a module.
        if qualified && self.is_type_param(&name.name) {
            return Head::Local;
        }
        if let Some(sym) = self.r.lookup(self.module, &name.name) {
            return Head::Symbol(sym);
        }
        if let Some((sym, idx)) = self.unqualified.get(&name.name).copied() {
            self.used_imports[idx] = true;
            return Head::Symbol(sym);
        }
        if let Some(sym) = self.r.names.get(&self.module).and_then(|n| n.picks.get(&name.name)) {
            return Head::Symbol(*sym);
        }
        if BUILTIN_TYPES.contains(&name.name.as_str())
            || (use_kind == PathUse::Value && BUILTIN_FNS.contains(&name.name.as_str()))
        {
            return Head::Builtin;
        }
        // Opened dependencies last: everything named explicitly wins, and among themselves
        // none does — two of them exporting the name is an error here (spec §3.2).
        match self.r.find_opened(self.module, &self.opened_modules(), &name.name) {
            crate::Opened::None => {}
            crate::Opened::One(sym) => {
                self.use_opened(sym);
                return Head::Symbol(sym);
            }
            crate::Opened::Ambiguous(found) => {
                self.ambiguous_opened(name, &found);
                return Head::Unresolved;
            }
        }
        // A single-segment name in value position may still be a local from an outer
        // construct the walker does not model yet; anything else is a real error.
        self.unresolved(name, use_kind);
        Head::Unresolved
    }

    fn check_members(&mut self, sym: SymbolId, rest: &[PathSegment]) {
        let mut current = sym;
        for seg in rest {
            let symbol = self.r.symbol(current);
            // A C++ namespace holds declarations, not members: the path goes on through it
            // for as many segments as it has (spec §15.2).
            if symbol.kind == crate::SymbolKind::Namespace {
                match self.r.inner(current, &seg.name.name) {
                    Some(id) => {
                        current = id;
                        continue;
                    }
                    None => {
                        let owner = symbol.name.clone();
                        let names: Vec<String> =
                            symbol.members.iter().map(|m| m.name.clone()).collect();
                        let suggestion = closest(&seg.name.name, names.iter().map(String::as_str));
                        let file = self.file;
                        let mut d = Diagnostic::error(
                            codes::NO_SUCH_MEMBER,
                            format!("namespace `{owner}` has no `{}`", seg.name.name),
                        )
                        .at(
                            file,
                            seg.name.span,
                            "not found in this namespace",
                        );
                        if let Some(s) = suggestion {
                            d = d.with_help(format!("did you mean `{s}`?"));
                        }
                        self.error(d);
                        return;
                    }
                }
            }
            if !symbol.kind.has_members() {
                let kind = symbol.kind.describe();
                let owner = symbol.name.clone();
                let file = self.file;
                self.error(
                    Diagnostic::error(
                        codes::NO_SUCH_MEMBER,
                        format!("`{owner}` is a {kind} and has no members"),
                    )
                    .at(
                        file,
                        seg.name.span,
                        format!("`{owner}` cannot be used with `::`"),
                    ),
                );
                return;
            }
            let Some(member) = symbol.member(&seg.name.name) else {
                // `Vec3::size` — a type answers the §5.5 queries as well as naming its
                // members. A member of the same name wins, which is why this comes second.
                let is_type = matches!(
                    symbol.kind,
                    crate::SymbolKind::Struct | crate::SymbolKind::Union | crate::SymbolKind::Enum
                );
                if is_type && crate::TYPE_QUERIES.contains(&seg.name.name.as_str()) {
                    return;
                }
                let owner = symbol.name.clone();
                let names: Vec<String> = symbol.members.iter().map(|m| m.name.clone()).collect();
                let suggestion = closest(&seg.name.name, names.iter().map(String::as_str));
                let file = self.file;
                let mut d = Diagnostic::error(
                    codes::NO_SUCH_MEMBER,
                    format!("`{owner}` has no member `{}`", seg.name.name),
                )
                .at(file, seg.name.span, "not found");
                if let Some(s) = suggestion {
                    d = d.with_help(format!("did you mean `{s}`?"));
                }
                self.error(d);
                return;
            };

            let member_vis = member.vis;
            let member_sort = member.sort;
            let member_span = member.span;
            let owner_module = symbol.module;
            let owner_name = symbol.name.clone();
            let member_name = member.name.clone();

            if !visible_from(member_vis, owner_module, self.module) {
                let file = self.file;
                let sort = member_sort.describe();
                self.error(
                    Diagnostic::error(
                        codes::PRIVATE_ITEM,
                        format!("{sort} `{owner_name}::{member_name}` is private"),
                    )
                    .at(file, seg.name.span, format!("this {sort} is not public"))
                    .with_label(dz_diag::Label::secondary(
                        self.r.symbol(current).file,
                        member_span,
                        "declared here",
                    ))
                    .with_help(format!(
                        "mark it `pub` to export it from module `{}` (spec §4)",
                        self.ws.module(owner_module).name
                    )),
                );
                return;
            }

            // Only a nested type could continue the path, and nothing nests yet.
            if member_sort != MemberSort::Variant {
                return;
            }
            current = sym;
        }
    }

    fn unknown_member_of_module(&mut self, target: ModuleId, name: &str, span: Span) {
        let module_name = self.ws.module(target).name.clone();
        let candidates: Vec<String> = self
            .r
            .scopes
            .get(&target)
            .map(|s| s.items.keys().cloned().collect())
            .unwrap_or_default();
        let suggestion = closest(name, candidates.iter().map(String::as_str));
        let file = self.file;
        let mut d = Diagnostic::error(
            codes::UNRESOLVED_NAME,
            format!("module `{module_name}` has no `{name}`"),
        )
        .at(file, span, "not found in this module");
        if let Some(s) = suggestion {
            d = d.with_help(format!("did you mean `{s}`?"));
        }
        self.error(d);
    }

    /// Report a leading segment that resolves to nothing.
    fn unresolved(&mut self, name: &Ident, use_kind: PathUse) {
        let what = match use_kind {
            PathUse::Type => "type",
            PathUse::Value => "value",
        };

        let mut candidates: Vec<String> = self
            .r
            .scopes
            .get(&self.module)
            .map(|s| s.items.keys().cloned().collect())
            .unwrap_or_default();
        candidates.extend(self.module_aliases.keys().cloned());
        candidates.extend(self.unqualified.keys().cloned());
        // An opened module's exports; a private name of the same spelling is noted instead,
        // since suggesting the very name that was written would help nobody.
        let mut hidden_in: Option<String> = None;
        for m in &self.opened_modules() {
            let Some(scope) = self.r.scopes.get(m) else { continue };
            for (item, id) in &scope.items {
                let s = self.r.symbol(*id);
                if visible_from(s.vis, s.module, self.module) {
                    candidates.push(item.clone());
                } else if *item == name.name {
                    hidden_in = Some(self.ws.module(*m).name.clone());
                }
            }
        }
        candidates.extend(self.scopes.iter().flat_map(|s| s.keys().cloned()));
        if use_kind == PathUse::Type {
            candidates.extend(BUILTIN_TYPES.iter().map(|s| (*s).to_owned()));
        } else {
            candidates.extend(BUILTIN_FNS.iter().map(|s| (*s).to_owned()));
        }
        let suggestion = closest(&name.name, candidates.iter().map(String::as_str));

        // A dependency that exports it, but is not opened — or, for the standard library, not
        // listed at all — says how to reach it rather than guessing at a spelling (spec §3.2).
        let mut in_dep: Option<(String, bool)> = None;
        for d in &self.ws.module(self.module).deps {
            let Some(id) = self.r.lookup(d.module, &name.name) else { continue };
            let s = self.r.symbol(id);
            if visible_from(s.vis, s.module, self.module) {
                in_dep = Some((d.alias.clone(), d.implicit));
                break;
            }
        }

        let file = self.file;
        let mut d = Diagnostic::error(
            codes::UNRESOLVED_NAME,
            format!("cannot find {what} `{}` in this scope", name.name),
        )
        .at(file, name.span, "not found");
        d = match (in_dep, suggestion) {
            (Some((_, true)), _) => d.with_help(format!(
                "`{}` is in the standard library, which this module's dazzle.toml does not \
                 list: add `std = {{ open = true }}` under `[dependencies]` (spec §14.4)",
                name.name
            )),
            (Some((key, false)), _) => d.with_help(format!(
                "write `{key}::{}`, or give `{key}` `open = true` in dazzle.toml to write it \
                 bare (spec §3.2)",
                name.name
            )),
            _ if hidden_in.is_some() => d.with_help(format!(
                "module `{}` has a `{}`, but it is not `pub`, so opening the dependency does \
                 not bring it in (spec §3.2, §4)",
                hidden_in.unwrap_or_default(),
                name.name
            )),
            (None, Some(s)) => d.with_help(format!("did you mean `{s}`?")),
            (None, None) => d.with_help(
                "declare it in this module, or list the module that exports it in dazzle.toml \
                 (spec §3.2)",
            ),
        };
        self.error(d);
    }

    fn check_item_visible(&mut self, sym: SymbolId, span: Span) -> bool {
        let symbol = self.r.symbol(sym);
        if visible_from(symbol.vis, symbol.module, self.module) {
            return true;
        }
        let name = symbol.name.clone();
        let kind = symbol.kind.describe();
        let decl_file = symbol.file;
        let decl_span = symbol.span;
        let owner_module = self.ws.module(symbol.module).name.clone();
        let file = self.file;
        self.error(
            Diagnostic::error(codes::PRIVATE_ITEM, format!("{kind} `{name}` is private"))
                .at(file, span, format!("not exported from module `{owner_module}`"))
                .with_label(dz_diag::Label::secondary(decl_file, decl_span, "declared here"))
                .with_help("add `pub` to the declaration to export it (spec §4)"),
        );
        false
    }
}

/// What an attribute is written on, as far as which attributes fit (spec §12.8).
#[derive(Clone, Copy)]
enum AttrTarget {
    ModuleFn,
    Static,
    /// Anything else, described for a message.
    Other(&'static str),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PathUse {
    Type,
    Value,
}

fn path_text(p: &Path) -> String {
    p.segments.iter().map(|s| s.name.name.as_str()).collect::<Vec<_>>().join("::")
}

fn last_segment(name: &str) -> &str {
    name.rsplit("::").next().unwrap_or(name)
}
