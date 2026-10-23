//! Compile-time conditionals: which flags are on, and the tree with only the branches they
//! select (spec §12.10).
//!
//! Runs after parsing and before resolution. Every branch was parsed — the formatter and
//! the editor need all of them — but only the active one is resolved, checked and emitted,
//! so an inactive branch may name what exists only on another platform or under another
//! flag. A condition naming a flag nobody registered is an error, not "off": a misspelt
//! `#if DEBUGG` that silently compiled nothing is exactly the mistake registration prevents.

use std::collections::{HashMap, HashSet};

use dz_ast::*;
use dz_diag::{codes, Diagnostic, Diagnostics};
use dz_project::{ModuleId, Optimize, Workspace, BUILTIN_FLAGS};
use dz_resolve::ParsedFile;

/// What a build turns on besides each package's own defaults.
#[derive(Clone, Debug, Default)]
pub struct BuildOptions {
    /// `--debug` / `--release` / `--shipping`, overriding every manifest's `[build] optimize`.
    pub optimize: Option<Optimize>,
    /// `--flag NAME` for the root package, `--flag engine::NAME` for a dependency.
    pub flags: Vec<String>,
    /// `--track-memory`: the prelude's `TRACK_MEMORY` (spec §14.2).
    pub track_memory: bool,
}

/// The flags that are on, by module. A built-in flag is on for every module it is true for.
pub struct Flags {
    on: HashMap<ModuleId, HashSet<String>>,
}

impl Flags {
    pub fn is_on(&self, module: ModuleId, name: &str) -> bool {
        self.on.get(&module).is_some_and(|s| s.contains(name))
    }

    /// Every flag on for `module`, sorted.
    pub fn on_for(&self, module: ModuleId) -> Vec<String> {
        let mut names: Vec<String> = self.on.get(&module).into_iter().flatten().cloned().collect();
        names.sort();
        names
    }
}

/// Settle every module's flags: its defaults, what its dependents ask of it, the command
/// line, and the built-ins for its build mode and the host.
pub fn resolve_flags(ws: &Workspace, opts: &BuildOptions, diags: &mut Diagnostics) -> Flags {
    let mut on: HashMap<ModuleId, HashSet<String>> = HashMap::new();
    for m in ws.modules() {
        let mut set: HashSet<String> =
            m.flags.iter().filter(|f| f.default).map(|f| f.name.clone()).collect();
        let optimize = opts.optimize.unwrap_or(m.build.optimize);
        set.insert(
            match optimize {
                Optimize::Debug => "DEBUG",
                Optimize::Release => "RELEASE",
                Optimize::Shipping => "SHIPPING",
            }
            .to_owned(),
        );
        set.insert(host_os_flag().to_owned());
        on.insert(m.id, set);
    }

    // A dependent turns a dependency's flags on: `engine = { path = "…", flags = ["X"] }`.
    for m in ws.modules() {
        for dep in &m.deps {
            let target = ws.module(dep.module);
            for (name, span) in &dep.flags {
                if target.flags.iter().any(|f| &f.name == name) {
                    on.entry(dep.module).or_default().insert(name.clone());
                } else {
                    diags.push(unknown_flag_in(ws, dep.module, name).at(
                        m.manifest,
                        *span,
                        format!("`{}` declares no such flag", dep.alias),
                    ));
                }
            }
        }
    }

    // The command line speaks for the root package.
    let root = ws.module(ws.root);
    for requested in &opts.flags {
        let (target, name) = match requested.rsplit_once("::") {
            Some((alias, name)) => match root.deps.iter().find(|d| d.alias == alias) {
                Some(d) => (d.module, name),
                None => {
                    diags.push(
                            Diagnostic::error(
                                codes::UNKNOWN_FLAG,
                                format!("`--flag {requested}`: `{alias}` is not a dependency of `{}`", root.name),
                            )
                            .with_help("name a dependency's flag by the name the root manifest gives it: `--flag engine::PROFILE` (spec §12.10)"),
                        );
                    continue;
                }
            },
            None => (ws.root, requested.as_str()),
        };
        if ws.module(target).flags.iter().any(|f| f.name == name) {
            on.entry(target).or_default().insert(name.to_owned());
        } else {
            diags.push(
                unknown_flag_in(ws, target, name).with_note(format!("from `--flag {requested}`")),
            );
        }
    }
    if opts.track_memory || root.build.track_memory {
        on.entry(ws.prelude).or_default().insert("TRACK_MEMORY".to_owned());
    }
    Flags { on }
}

fn unknown_flag_in(ws: &Workspace, module: ModuleId, name: &str) -> Diagnostic {
    let m = ws.module(module);
    let names: Vec<&str> = m.flags.iter().map(|f| f.name.as_str()).collect();
    let mut d =
        Diagnostic::error(codes::UNKNOWN_FLAG, format!("`{}` has no flag `{name}`", m.name));
    d = match dz_resolve::closest(name, names.iter().copied()) {
        Some(s) => d.with_help(format!("did you mean `{s}`?")),
        None => d.with_help(format!(
            "a flag is declared under `[flags]` in `{}`'s dazzle.toml (spec §12.10)",
            m.name
        )),
    };
    d
}

fn host_os_flag() -> &'static str {
    if cfg!(target_os = "windows") {
        "OS_WINDOWS"
    } else if cfg!(target_os = "macos") {
        "OS_MACOS"
    } else {
        "OS_LINUX"
    }
}

/// Replace every `#if` in a module's files with its active branch's contents.
pub fn strip(
    ws: &Workspace,
    flags: &Flags,
    module: ModuleId,
    files: &mut [ParsedFile],
    diags: &mut Diagnostics,
) {
    for pf in files.iter_mut() {
        let mut s = Stripper { ws, flags, module, file: pf.file, diags };
        let items = std::mem::take(&mut pf.ast.items);
        pf.ast.items = s.items(items);
    }
}

struct Stripper<'a> {
    ws: &'a Workspace,
    flags: &'a Flags,
    module: ModuleId,
    file: dz_span::FileId,
    diags: &'a mut Diagnostics,
}

impl Stripper<'_> {
    /// The branch a chain selects, if any.
    fn select<T>(&mut self, c: CfgIf<T>) -> Vec<T> {
        // Every condition in the chain is evaluated, taken or not, so a misspelt flag in an
        // `#else #if` is reported even when an earlier branch won.
        let mut chosen: Option<Vec<T>> = None;
        let mut next = Some(c);
        while let Some(c) = next {
            let on = self.eval(&c.cond);
            if chosen.is_none() && on {
                chosen = Some(c.then);
            }
            next = match c.otherwise {
                Some(CfgElse::If(n)) => Some(*n),
                Some(CfgElse::Else { items, .. }) => {
                    if chosen.is_none() {
                        chosen = Some(items);
                    }
                    None
                }
                None => None,
            };
        }
        chosen.unwrap_or_default()
    }

    fn eval(&mut self, c: &CfgCond) -> bool {
        match &c.kind {
            CfgCondKind::Flag(p) => self.flag(p),
            CfgCondKind::Not(x) => !self.eval(x),
            // Both sides, always, for the same reason as `select`.
            CfgCondKind::And(a, b) => {
                let (a, b) = (self.eval(a), self.eval(b));
                a && b
            }
            CfgCondKind::Or(a, b) => {
                let (a, b) = (self.eval(a), self.eval(b));
                a || b
            }
            CfgCondKind::Paren(x) => self.eval(x),
            CfgCondKind::Error => false,
        }
    }

    /// `DEBUG`, `TRACK` for this package's own, `engine::PROFILE` for a dependency's.
    fn flag(&mut self, p: &Path) -> bool {
        let names: Vec<&str> = p.segments.iter().map(|s| s.name.name.as_str()).collect();
        let (target, name) = match names[..] {
            [name] if BUILTIN_FLAGS.contains(&name) => return self.flags.is_on(self.module, name),
            [name] => (self.module, name),
            [alias, name] => {
                let m = self.ws.module(self.module);
                match m.deps.iter().find(|d| d.alias == alias) {
                    Some(d) => (d.module, name),
                    None => {
                        self.diags.push(
                            Diagnostic::error(
                                codes::UNKNOWN_FLAG,
                                format!("`{alias}` is not a dependency of `{}`", m.name),
                            )
                            .at(self.file, p.segments[0].span, "not a dependency")
                            .with_help("a dependency's flag is named through the dependency: `#if engine::PROFILE` (spec §12.10)"),
                        );
                        return false;
                    }
                }
            }
            _ => {
                self.diags.push(
                    Diagnostic::error(
                        codes::UNKNOWN_FLAG,
                        "a flag is `NAME` or `dependency::NAME`",
                    )
                    .at(self.file, p.span, "too many segments")
                    .with_help("spec §12.10"),
                );
                return false;
            }
        };
        let declared = self.ws.module(target).flags.iter().any(|f| f.name == name);
        if !declared {
            // A near-miss of a built-in is the likelier intent than a flag nobody declared.
            let builtin = (target == self.module)
                .then(|| dz_resolve::closest(name, BUILTIN_FLAGS.iter().copied()))
                .flatten();
            let d = match builtin {
                Some(b) => Diagnostic::error(
                    codes::UNKNOWN_FLAG,
                    format!("`{}` has no flag `{name}`", self.ws.module(target).name),
                )
                .with_help(format!("did you mean the built-in `{b}`?")),
                None => unknown_flag_in(self.ws, target, name),
            };
            self.diags.push(d.at(self.file, p.span, "not a registered flag"));
            return false;
        }
        self.flags.is_on(target, name)
    }

    // --- the walk ---------------------------------------------------------------

    fn items(&mut self, items: Vec<Item>) -> Vec<Item> {
        let mut out = Vec::with_capacity(items.len());
        for mut item in items {
            if let ItemKind::CfgIf(c) = item.kind {
                let chosen = self.select(c);
                out.extend(self.items(chosen));
                continue;
            }
            self.item(&mut item);
            out.push(item);
        }
        out
    }

    fn item(&mut self, item: &mut Item) {
        match &mut item.kind {
            ItemKind::Fn(f) => self.fn_decl(f),
            ItemKind::Struct(s) => s.members = self.members(std::mem::take(&mut s.members)),
            ItemKind::Trait(t) => t.items = self.members(std::mem::take(&mut t.items)),
            ItemKind::Impl(i) => i.items = self.members(std::mem::take(&mut i.items)),
            ItemKind::Const(c) => self.expr(&mut c.value),
            ItemKind::Static(s) => self.expr(&mut s.value),
            ItemKind::Enum(e) => {
                for v in &mut e.variants {
                    if let Some(x) = &mut v.value {
                        self.expr(x);
                    }
                }
            }
            ItemKind::Union(_)
            | ItemKind::Macro(_)
            | ItemKind::MacroInvocation(_)
            | ItemKind::CfgIf(_)
            | ItemKind::Error => {}
        }
    }

    fn members(&mut self, members: Vec<Member>) -> Vec<Member> {
        let mut out = Vec::with_capacity(members.len());
        for mut m in members {
            if let MemberKind::CfgIf(c) = m.kind {
                let chosen = self.select(c);
                out.extend(self.members(chosen));
                continue;
            }
            match &mut m.kind {
                MemberKind::Fn(f) | MemberKind::Ctor(f) => self.fn_decl(f),
                MemberKind::Field(f) => {
                    if let Some(d) = &mut f.default {
                        self.expr(d);
                    }
                }
                MemberKind::Const(c) => self.expr(&mut c.value),
                MemberKind::TypeAlias { .. } | MemberKind::CfgIf(_) | MemberKind::Error => {}
            }
            out.push(m);
        }
        out
    }

    fn fn_decl(&mut self, f: &mut FnDecl) {
        for p in &mut f.params {
            if let Some(d) = &mut p.default {
                self.expr(d);
            }
        }
        if let Some(b) = &mut f.base_init {
            for a in &mut b.args {
                self.expr(a);
            }
        }
        if let Some(body) = &mut f.body {
            self.block(body);
        }
    }

    fn block(&mut self, b: &mut Block) {
        b.stmts = self.stmts(std::mem::take(&mut b.stmts));
    }

    fn stmts(&mut self, stmts: Vec<Stmt>) -> Vec<Stmt> {
        let mut out = Vec::with_capacity(stmts.len());
        for mut s in stmts {
            if let StmtKind::CfgIf(c) = s.kind {
                let chosen = self.select(c);
                out.extend(self.stmts(chosen));
                continue;
            }
            self.stmt(&mut s);
            out.push(s);
        }
        out
    }

    fn stmt(&mut self, s: &mut Stmt) {
        match &mut s.kind {
            StmtKind::Let(l) => {
                if let Some(i) = &mut l.init {
                    self.expr(i);
                }
            }
            StmtKind::Assign { target, value, .. } => {
                self.expr(target);
                self.expr(value);
            }
            StmtKind::Expr(e) => self.expr(e),
            StmtKind::While(w) => {
                self.expr(&mut w.cond);
                self.block(&mut w.body);
            }
            StmtKind::For(f) => {
                self.expr(&mut f.iter);
                self.block(&mut f.body);
            }
            StmtKind::Defer(inner) => self.stmt(inner),
            StmtKind::Item(item) => {
                // A nested `#if` item was spliced by the statement list; this is one item.
                self.item(item);
            }
            StmtKind::CfgIf(_) | StmtKind::Macro(_) | StmtKind::Error => {}
        }
    }

    fn expr(&mut self, e: &mut Expr) {
        match &mut e.kind {
            ExprKind::DotInfer { init: Some(fields), .. } | ExprKind::StructLit { fields, .. } => {
                for f in fields {
                    self.expr(&mut f.value);
                }
            }
            ExprKind::ArrayLit(xs) | ExprKind::Tuple(xs) => {
                for x in xs {
                    self.expr(x);
                }
            }
            ExprKind::Call { callee, args } => {
                self.expr(callee);
                for a in args {
                    self.expr(a);
                }
            }
            ExprKind::Intrinsic { args, .. } => {
                for a in args {
                    self.expr(a);
                }
            }
            ExprKind::Index { base, index } => {
                self.expr(base);
                self.expr(index);
            }
            ExprKind::Field { base, .. } => self.expr(base),
            ExprKind::Unary { operand, .. } => self.expr(operand),
            ExprKind::Binary { lhs, rhs, .. } => {
                self.expr(lhs);
                self.expr(rhs);
            }
            ExprKind::Cast { expr, .. }
            | ExprKind::Ref { expr, .. }
            | ExprKind::Deref(expr)
            | ExprKind::Move(expr) => self.expr(expr),
            ExprKind::Range { start, end, .. } => {
                for x in [start, end].into_iter().flatten() {
                    self.expr(x);
                }
            }
            ExprKind::Or { expr, clause } => {
                self.expr(expr);
                match clause.as_mut() {
                    OrClause::Value(v) => self.expr(v),
                    OrClause::Block { body, .. } => self.block(body),
                    OrClause::Ignore(_) | OrClause::Unreachable(_) => {}
                }
            }
            ExprKind::Block(b) | ExprKind::Unsafe(b) => self.block(b),
            ExprKind::If(i) => {
                self.expr(&mut i.cond);
                self.block(&mut i.then);
                if let Some(e) = &mut i.else_branch {
                    self.expr(e);
                }
            }
            ExprKind::Switch(s) => {
                self.expr(&mut s.subject);
                for arm in &mut s.arms {
                    for pat in &mut arm.patterns {
                        if let CasePat::Value(v) = pat {
                            self.expr(v);
                        }
                    }
                    self.block(&mut arm.body);
                }
                if let Some(d) = &mut s.default {
                    self.block(d);
                }
            }
            ExprKind::Return(Some(v)) => self.expr(v),
            _ => {}
        }
    }
}
