//! Hygiene, and names that resolve where the macro is declared (spec §12.4).
//!
//! A token the macro wrote has the invocation's name as its span ([`crate::template`]), so
//! after parsing, "did the macro introduce this name?" is a span comparison. Two passes
//! over the expansion then apply §12.4:
//!
//! 1. **Locals the macro introduced are renamed** to `name·N`, a spelling no source can
//!    write, so they can neither capture nor be captured by the caller's names. Only
//!    *locals* are: a type, function, method, field or variant the macro declares keeps its
//!    name, because the caller is meant to use it.
//! 2. **A name the macro wrote that its own module declares** is qualified with the alias
//!    the caller reached the macro through, so `#engine::machine` expanding to
//!    `StateMachine` means `engine::StateMachine`.
//!
//! Renaming is by name within one expansion rather than by scope: a macro that writes the
//! same identifier as both a local and a module-level name would see both renamed. Nothing
//! in a well-written macro does that, and it costs no scope tracking before resolution.

use std::collections::HashSet;

use dz_ast::*;
use dz_span::Span;

use crate::walk::{self, Hooks};

/// Where an expansion's output is, for the passes to walk.
pub(crate) enum Output<'a> {
    Items(&'a mut [Item]),
    Stmts(&'a mut Vec<Stmt>),
    Expr(&'a mut Expr),
}

pub(crate) struct DefSite<'a> {
    /// The alias the caller wrote, `engine` in `#engine::machine`.
    pub alias: &'a str,
    /// What the macro's module declares at its top level.
    pub names: &'a HashSet<String>,
}

pub(crate) fn apply(out: Output<'_>, name_span: Span, serial: u32, def: Option<DefSite<'_>>) {
    let mut collect = Collect { name_span, binders: HashSet::new() };
    let mut rename = Rename { name_span, serial, binders: HashSet::new(), def };
    match out {
        Output::Items(items) => {
            walk::items(&mut collect, items);
            rename.binders = collect.binders;
            walk::items(&mut rename, items);
            walk::items(&mut Widen, items);
        }
        Output::Stmts(stmts) => {
            walk::stmts(&mut collect, stmts);
            rename.binders = collect.binders;
            walk::stmts(&mut rename, stmts);
            walk::stmts(&mut Widen, stmts);
        }
        Output::Expr(e) => {
            walk::expr(&mut collect, e);
            rename.binders = collect.binders;
            walk::expr(&mut rename, e);
            walk::expr(&mut Widen, e);
        }
    }
}

/// Make each block and statement cover the code inside it.
///
/// A block the macro wrote has its braces at the invocation's name, so its span is just
/// `#machine` — though what it holds may be a state's body, many lines below. The editor
/// finds the function around the cursor by its body's span (spec §17.4), so a body must
/// cover the captured code in it for completion there to see its locals.
struct Widen;

impl Hooks for Widen {
    fn block_done(&mut self, b: &mut Block) {
        for s in &b.stmts {
            b.span = b.span.to(s.span);
        }
    }

    fn stmt_done(&mut self, s: &mut Stmt) {
        let mut cover = s.span;
        let mut add = |b: &Block| cover = cover.to(b.span);
        match &s.kind {
            StmtKind::While(w) => add(&w.body),
            StmtKind::For(f) => add(&f.body),
            StmtKind::Let(LetStmt { init: Some(e), .. }) | StmtKind::Expr(e) => {
                expr_blocks(e, &mut add)
            }
            _ => {}
        }
        s.span = cover;
    }
}

/// The blocks directly inside an expression a statement is made of.
fn expr_blocks(e: &Expr, add: &mut impl FnMut(&Block)) {
    match &e.kind {
        ExprKind::Block(b) | ExprKind::Unsafe(b) => add(b),
        ExprKind::If(i) => {
            add(&i.then);
            if let Some(x) = &i.else_branch {
                expr_blocks(x, add);
            }
        }
        ExprKind::Switch(sw) => {
            for arm in &sw.arms {
                add(&arm.body);
            }
            if let Some(d) = &sw.default {
                add(d);
            }
        }
        ExprKind::Or { clause, .. } => {
            if let OrClause::Block { body, .. } = clause.as_ref() {
                add(body);
            }
        }
        _ => {}
    }
}

/// Is `name` a name hygiene gave? A warning about such a local is the macro author's to
/// fix, not the user's (spec §12.6).
pub fn is_hygienic(name: &str) -> bool {
    name.contains('·')
}

struct Collect {
    name_span: Span,
    binders: HashSet<String>,
}

impl Hooks for Collect {
    fn binder(&mut self, id: &mut Ident) {
        if id.span == self.name_span {
            self.binders.insert(id.name.clone());
        }
    }
}

struct Rename<'a> {
    name_span: Span,
    serial: u32,
    binders: HashSet<String>,
    def: Option<DefSite<'a>>,
}

impl Rename<'_> {
    /// The macro's own local of this name, renamed if it is one.
    fn local(&self, id: &Ident) -> Option<String> {
        (id.span == self.name_span && self.binders.contains(&id.name))
            .then(|| format!("{}·{}", id.name, self.serial))
    }

    /// `StateMachine` → `engine::StateMachine`, when the macro wrote it and its module
    /// declares it.
    fn qualify(&self, p: &mut Path) {
        let Some(def) = &self.def else { return };
        let Some(first) = p.segments.first() else { return };
        if first.name.span != self.name_span
            || self.binders.contains(&first.name.name)
            || !def.names.contains(&first.name.name)
        {
            return;
        }
        let alias = PathSegment {
            span: self.name_span,
            name: Ident::new(self.name_span, def.alias),
            generics: Vec::new(),
        };
        p.segments.insert(0, alias);
    }
}

impl Hooks for Rename<'_> {
    fn binder(&mut self, id: &mut Ident) {
        if let Some(n) = self.local(id) {
            id.name = n;
        }
    }

    fn value_path(&mut self, p: &mut Path) {
        if let [seg] = p.segments.as_mut_slice() {
            if let Some(n) = self.local(&seg.name) {
                seg.name.name = n;
                return;
            }
        }
        self.qualify(p);
    }

    fn type_path(&mut self, p: &mut Path) {
        self.qualify(p);
    }

    fn field_init(&mut self, f: &mut FieldInit) {
        // `Foo{ x }` names its field by the expression: renaming the local must not rename
        // the field, so the shorthand becomes `Foo{ x: x·1 }`.
        if f.name.is_none() {
            if let ExprKind::Path(p) = &f.value.kind {
                if let [seg] = p.segments.as_slice() {
                    if self.local(&seg.name).is_some() {
                        f.name = Some(seg.name.clone());
                    }
                }
            }
        }
    }
}
