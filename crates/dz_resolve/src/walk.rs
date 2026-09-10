//! The resolution walk: imports, then every type and expression in the file.

use std::collections::HashMap;

use dz_ast::*;
use dz_diag::{codes, Diagnostic};
use dz_project::{ModuleId, Workspace};
use dz_span::{FileId, Span};

use crate::{
    closest, visible_from, MemberSort, ParsedFile, Resolution, SymbolId, BUILTIN_FNS,
    BUILTIN_TYPES,
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

pub(crate) fn resolve_file(
    r: &mut Resolution,
    ws: &Workspace,
    module: ModuleId,
    pf: &ParsedFile,
) {
    let mut w = Walker {
        r: &mut *r,
        ws,
        module,
        file: pf.file,
        module_aliases: HashMap::new(),
        unqualified: HashMap::new(),
        scopes: Vec::new(),
        used_imports: Vec::new(),
        imports: Vec::new(),
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
    /// `import engine::render as gfx` → "gfx" → that module.
    module_aliases: HashMap<String, (ModuleId, usize)>,
    /// `import math::{ pow }` → "pow" → that symbol.
    unqualified: HashMap<String, (SymbolId, usize)>,
    /// Innermost scope last. Each holds local binding names.
    scopes: Vec<HashMap<String, Span>>,
    used_imports: Vec<bool>,
    imports: Vec<(String, Span)>,
}

impl<'a, 'w> Walker<'a, 'w> {
    fn error(&mut self, d: Diagnostic) {
        self.r.diagnostics.push(d);
    }

    // --- imports ----------------------------------------------------------

    fn imports(&mut self, ast: &SourceFileAst) {
        for import in &ast.imports {
            let index = self.imports.len();
            match &import.kind {
                ImportKind::Module { path, alias } => {
                    let name = path_text(path);
                    let Some(target) = self.find_module(&name, path.span) else { continue };
                    let local = alias
                        .as_ref()
                        .map(|a| a.name.clone())
                        .unwrap_or_else(|| last_segment(&name).to_owned());
                    self.imports.push((local.clone(), import.span));
                    self.used_imports.push(false);
                    self.module_aliases.insert(local, (target, index));
                }
                ImportKind::Selective { path, names } => {
                    let mod_name = path_text(path);
                    let Some(target) = self.find_module(&mod_name, path.span) else { continue };
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
                                .at(file, n.span, format!("`{prev_name}` is already in scope")),
                            );
                            continue;
                        }
                        self.unqualified.insert(n.name.clone(), (sym, index));
                    }
                }
                // C++ headers are not read until M7; the alias is accepted as opaque.
                ImportKind::Cpp { .. } | ImportKind::Error => {}
            }
        }
    }

    /// A module path names either this module's own name or one of its dependencies.
    fn find_module(&mut self, name: &str, span: Span) -> Option<ModuleId> {
        let me = self.ws.module(self.module);
        if me.name == name {
            return Some(self.module);
        }
        // The `[dependencies]` key is the name used in `import` (spec §3.1)...
        if let Some(dep) = me.dependency(name) {
            return Some(dep.module);
        }
        // ...and so is the dependency's own declared name, so `import engine::render`
        // works whatever the manifest called the entry.
        for dep in &me.deps {
            if self.ws.module(dep.module).name == name {
                return Some(dep.module);
            }
        }

        let known: Vec<String> = me
            .deps
            .iter()
            .map(|d| self.ws.module(d.module).name.clone())
            .chain(std::iter::once(me.name.clone()))
            .collect();
        let suggestion = closest(name, known.iter().map(String::as_str));
        let file = self.file;
        let mut d = Diagnostic::error(
            codes::UNKNOWN_MODULE,
            format!("no module `{name}` in scope"),
        )
        .at(file, span, "not a dependency of this module");
        d = match suggestion {
            Some(s) => d.with_help(format!("did you mean `{s}`?")),
            None if known.len() > 1 => d.with_help(format!(
                "available modules are {}",
                known.iter().map(|k| format!("`{k}`")).collect::<Vec<_>>().join(", ")
            )),
            None => d.with_help(
                "add it to `[dependencies]` in this module's dazzle.toml (spec §3.1)",
            ),
        };
        self.error(d);
        None
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
        match &item.kind {
            ItemKind::Fn(f) => self.function(f, None),
            ItemKind::Const(c) => {
                if let Some(t) = &c.ty {
                    self.ty(t);
                }
                self.push_scope();
                self.expr(&c.value);
                self.pop_scope();
            }
            ItemKind::Struct(s) => {
                // A type's generic parameters are in scope for its own members.
                self.push_scope();
                self.bind_generics(&s.generics);
                for b in &s.traits {
                    self.path(b, PathUse::Type);
                }
                self.members(&s.members, &s.name.name, Some(&s.members));
                self.pop_scope();
            }
            ItemKind::Trait(t) => {
                self.push_scope();
                self.bind_generics(&t.generics);
                for b in &t.supertraits {
                    self.path(b, PathUse::Type);
                }
                self.members(&t.items, &t.name.name, None);
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
            // Macro bodies are not expanded until M10.
            ItemKind::Macro(_) | ItemKind::MacroInvocation { .. } | ItemKind::Error => {}
        }
    }

    fn members(&mut self, members: &[Member], _owner: &str, fields_in_scope: Option<&[Member]>) {
        for m in members {
            match &m.kind {
                MemberKind::Field(f) => self.field(f),
                MemberKind::Fn(f)
                | MemberKind::Ctor(f)
                | MemberKind::Copy(f)
                | MemberKind::Drop(f) => self.function(f, fields_in_scope),
                MemberKind::TypeAlias { ty, .. } => {
                    if let Some(t) = ty {
                        self.ty(t);
                    }
                }
                MemberKind::Error => {}
            }
        }
    }

    /// Bring a declaration's generic parameters into scope as type names.
    fn bind_generics(&mut self, g: &Generics) {
        for p in &g.params {
            self.bind(&p.name.name, p.name.span);
            for b in &p.bounds {
                self.path(b, PathUse::Type);
            }
        }
    }

    fn field(&mut self, f: &Field) {
        self.ty(&f.ty);
        if let Some(d) = &f.default {
            self.push_scope();
            self.expr(d);
            self.pop_scope();
        }
    }

    fn function(&mut self, f: &FnDecl, fields_in_scope: Option<&[Member]>) {
        self.push_scope();

        self.bind_generics(&f.generics);

        // Inside a method, the struct's fields are in scope unqualified (spec §7.3).
        // They go in their own scope so a parameter of the same name shadows the field
        // rather than colliding with it.
        if let Some(members) = fields_in_scope {
            self.push_scope();
            if let Some(scope) = self.scopes.last_mut() {
                scope.insert("self".to_owned(), Span::DUMMY);
            }
            for m in members {
                if let MemberKind::Field(field) = &m.kind {
                    if let Some(scope) = self.scopes.last_mut() {
                        scope.entry(field.name.name.clone()).or_insert(field.name.span);
                    }
                }
            }
        }
        self.push_scope();

        for p in &f.params {
            self.ty(&p.ty);
            if let Some(d) = &p.default {
                self.expr(d);
            }
            self.bind(&p.name.name, p.name.span);
        }
        if let Some(ret) = &f.ret {
            self.ty(ret);
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
            TypeKind::Ptr(i) | TypeKind::OwnPtr(i) | TypeKind::Optional(i) => self.ty(i),
            TypeKind::Ref { inner, .. } => self.ty(inner),
            TypeKind::Array { size, elem } => {
                if let Some(s) = size {
                    self.expr(s);
                }
                self.ty(elem);
            }
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
                    self.bind_pattern(b);
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
            ExprKind::Field { base, .. } => self.expr(base),
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
            ExprKind::Ref { expr, .. } | ExprKind::Deref(expr) | ExprKind::Move(expr) => {
                self.expr(expr)
            }
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
            ExprKind::Block(b) => self.block(b),
            ExprKind::If(i) => {
                self.expr(&i.cond);
                self.block(&i.then);
                if let Some(e) = &i.else_branch {
                    self.expr(e);
                }
            }
            ExprKind::Switch(s) => {
                self.expr(&s.subject);
                for arm in &s.arms {
                    self.push_scope();
                    for p in &arm.patterns {
                        if let Some(b) = &p.binding {
                            self.bind(&b.name, b.span);
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
            ExprKind::Break { value, .. } => {
                if let Some(v) = value {
                    self.expr(v);
                }
            }
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
            | ExprKind::MacroCall { .. }
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
            Head::Module(target) => {
                let Some(second) = p.segments.get(1) else {
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
                self.check_members(sym, &p.segments[2..]);
            }
            Head::Symbol(sym) => self.check_members(sym, &p.segments[1..]),
        }
    }

    fn resolve_head(&mut self, name: &Ident, use_kind: PathUse, qualified: bool) -> Head {
        if !qualified && self.is_local(&name.name) {
            return Head::Local;
        }
        if let Some((m, idx)) = self.module_aliases.get(&name.name).copied() {
            self.used_imports[idx] = true;
            return Head::Module(m);
        }
        if let Some(sym) = self.r.lookup(self.module, &name.name) {
            return Head::Symbol(sym);
        }
        if let Some((sym, idx)) = self.unqualified.get(&name.name).copied() {
            self.used_imports[idx] = true;
            return Head::Symbol(sym);
        }
        if BUILTIN_TYPES.contains(&name.name.as_str())
            || (use_kind == PathUse::Value && BUILTIN_FNS.contains(&name.name.as_str()))
        {
            return Head::Builtin;
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
            if !symbol.kind.has_members() {
                let kind = symbol.kind.describe();
                let owner = symbol.name.clone();
                let file = self.file;
                self.error(
                    Diagnostic::error(
                        codes::NO_SUCH_MEMBER,
                        format!("`{owner}` is a {kind} and has no members"),
                    )
                    .at(file, seg.name.span, format!("`{owner}` cannot be used with `::`")),
                );
                return;
            }
            let Some(member) = symbol.member(&seg.name.name) else {
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
        candidates.extend(self.scopes.iter().flat_map(|s| s.keys().cloned()));
        if use_kind == PathUse::Type {
            candidates.extend(BUILTIN_TYPES.iter().map(|s| (*s).to_owned()));
        } else {
            candidates.extend(BUILTIN_FNS.iter().map(|s| (*s).to_owned()));
        }
        let suggestion = closest(&name.name, candidates.iter().map(String::as_str));

        let file = self.file;
        let mut d = Diagnostic::error(
            codes::UNRESOLVED_NAME,
            format!("cannot find {what} `{}` in this scope", name.name),
        )
        .at(file, name.span, "not found");
        d = match suggestion {
            Some(s) => d.with_help(format!("did you mean `{s}`?")),
            None => d.with_help(
                "declare it in this module, or `import` the module that exports it (spec §3.2)",
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
