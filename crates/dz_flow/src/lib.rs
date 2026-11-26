//! Ownership and flow analysis (spec §6).
//!
//! Everything here is a consequence of one sentence in §6.3: a type that owns a resource
//! is **move-only**, and every other rule is about keeping track of where the one live
//! copy is. Six things are checked:
//!
//! * a move-only value used where a copy would be needed (`DZ0403`);
//! * a value read after it was moved out of, on any path (`DZ0401`, `DZ0404`);
//! * `move` out of somewhere the function does not own (`DZ0402`);
//! * a value-returning function with a path that falls off the end (`DZ0405`);
//! * a binding nothing reads — a warning, not an error (`DZ0406`);
//! * a view kept from a temporary that is dropped at the end of its statement (`DZ0407`).
//!
//! **This pass reads the HIR and nothing else.** It needs a name to be a `LocalId` so a
//! place can be a map key, and every expression to carry its type so "is this move-only?"
//! is a lookup rather than an inference — which is exactly what §17.1 says the HIR is for.
//!
//! The analysis is *structured*, not a CFG: it walks the tree, threading a set of moved
//! places and merging at each join with a union. Union is the right merge because spec
//! §6.4 says a value moved on *some* path reaching a use is an error there, not only one
//! moved on all of them. That is stricter than a may/must distinction would be, and it is
//! the answer that produces the diagnostic the programmer needs.

use std::collections::HashMap;

use dz_check::TypeTable;
use dz_diag::{codes, Diagnostic, Diagnostics};
use dz_hir::ty::Ty;
use dz_hir::{self as hir, BlockId, ExprId, Hir, LocalId, StmtId};
use dz_resolve::Resolution;
use dz_span::{FileId, SourceMap, Span};

pub mod arenas;
pub mod placement;
pub mod release;
pub(crate) mod storing;
pub mod ties;
pub mod views;

/// Check every body in the program.
///
/// `map` is read only to lay out a fix's edit, as in `dz_check::check`: a tie the fix
/// declares goes on a line of its own among the type's members (spec §6.7).
pub fn check(hir: &Hir, table: &TypeTable, res: &Resolution, map: &SourceMap) -> Diagnostics {
    check_files(hir, table, res, map, None)
}

/// [`check`], for the bodies written in `only`'s files, or every body for `None`. A body is
/// checked against other functions' signatures alone, so the bodies of a module nothing
/// changed for need no checking again (T-0197).
///
/// `std`'s bodies are never checked here. They are the same in every program, and a body is
/// checked against signatures alone, so nothing a program declares can change what they
/// say; checking them was about a third of a small program's analysis (T-0025). The test
/// suite runs [`check_std`] instead, with each of `std`'s `#if` branches.
pub fn check_files(
    hir: &Hir,
    table: &TypeTable,
    res: &Resolution,
    map: &SourceMap,
    only: Option<&std::collections::HashSet<FileId>>,
) -> Diagnostics {
    let prelude = prelude_files(res);
    let mut diags = Diagnostics::new();
    let mut unread = Unread::default();
    for (_, body) in hir.bodies() {
        if prelude.contains(&body.file) || only.is_some_and(|only| !only.contains(&body.file)) {
            continue;
        }
        check_body(body, table, res, map, &mut diags);
        // A trait method's parameters are declarations, not bindings anything could read:
        // its body exists only to hold their defaults (spec §7.6).
        if !matches!(body.owner, dz_hir::BodyOwner::TraitDefaults { .. }) {
            unused(body, &mut unread);
        }
    }
    unread.report(&mut diags);
    diags.sort_by_position();
    diags
}

/// What [`check_files`] leaves out: every check of `std`'s own bodies but the unread
/// bindings, which are the compiler's business and noise in every program. The test suite
/// runs it, so a mistake in `std` fails there rather than in every build.
pub fn check_std(hir: &Hir, table: &TypeTable, res: &Resolution, map: &SourceMap) -> Diagnostics {
    let prelude = prelude_files(res);
    let mut diags = Diagnostics::new();
    for (_, body) in hir.bodies() {
        if prelude.contains(&body.file) {
            check_body(body, table, res, map, &mut diags);
        }
    }
    diags.sort_by_position();
    diags
}

/// The files of the prelude, `std`.
fn prelude_files(res: &Resolution) -> std::collections::HashSet<FileId> {
    res.symbols.iter().filter(|s| s.module == res.prelude).map(|s| s.file).collect()
}

/// Every check of one body but the unread bindings, which are gathered over all of them.
fn check_body(
    body: &hir::Body,
    table: &TypeTable,
    res: &Resolution,
    map: &SourceMap,
    diags: &mut Diagnostics,
) {
    let mut cx = Flow {
        body,
        table,
        res,
        file: body.file,
        diags,
        moved: HashMap::new(),
        cloneable: std::collections::HashSet::new(),
        quiet: false,
        loops: Vec::new(),
    };
    cx.body();
    // References used after their memory was killed, and clauses that do not say what
    // the body kills (spec §6.7).
    let analysis = ties::analyse_body(body, table, res);
    ties::report(body, table, res, map, &analysis, diags);
    // What the body does with its inputs' memory, against `sends` and `in x@home` (D-0097).
    let homes = ties::analyse_homes(body, table, res);
    ties::report_homes(body, table, res, &homes, diags);
}

/// Unread bindings, gathered over every body before any is reported.
///
/// A macro may copy one parameter declaration into several functions — `#machine`'s
/// parameters become every state's `update`'s. Each copy is a binding of its own, at the
/// same span, and the declaration is unused only if every copy is (spec §12.6, 18.79).
#[derive(Default)]
struct Unread {
    /// Whether some binding at this place is read.
    read: HashMap<(FileId, Span), bool>,
    /// The warning for each place, in the order first found.
    pending: Vec<((FileId, Span), Diagnostic)>,
}

impl Unread {
    fn report(self, diags: &mut Diagnostics) {
        let mut done = std::collections::HashSet::new();
        for (key, d) in self.pending {
            if !self.read.get(&key).copied().unwrap_or(false) && done.insert(key) {
                diags.push(d);
            }
        }
    }
}

/// A warning for each binding nothing reads (`DZ0406`). The program is still correct, so
/// it is not an error; but an unread `let` is usually a leftover or a typo, and a parameter
/// nothing reads is a signature worth a second look.
fn unused(body: &hir::Body, unread: &mut Unread) {
    for (i, (l, u)) in body.locals.iter().zip(hir::local_uses(body)).enumerate() {
        // `self` is part of a method's shape; a leading `_` says "unused on purpose"; a
        // local whose span is not its own name — an implicit `it` — was written by no one;
        // and one a macro's hygiene renamed is the macro author's (spec §12.6).
        let written = l.span.len() as usize == l.name.len();
        let key = (body.file, l.span);
        if u.read {
            unread.read.insert(key, true);
            continue;
        }
        if l.name == "self" || l.name.starts_with('_') || !written || l.name.contains('·') {
            continue;
        }
        unread.read.entry(key).or_insert(false);
        let what = if l.kind == hir::LocalKind::Param { "parameter" } else { "variable" };
        let d = if u.assigned {
            Diagnostic::warning(
                codes::UNUSED_VARIABLE,
                format!("`{}` is assigned but never read", l.name),
            )
            .at(body.file, l.span, "every value given to it is thrown away")
        } else {
            Diagnostic::warning(
                codes::UNUSED_VARIABLE,
                format!("{what} `{}` is never used", l.name),
            )
            .at(body.file, l.span, "nothing reads this")
        };
        // The span is the name exactly as written (`written` above), so a `_` in front of it
        // renames it: a leading `_` is what says "unused on purpose". A variable that is
        // assigned is named again at each assignment, and each is renamed with it, or the
        // fix would leave them naming nothing (T-0108).
        let underscore = |span: dz_span::Span| dz_diag::Edit {
            file: body.file,
            span: dz_span::Span { start: span.start, end: span.start },
            text: "_".to_owned(),
        };
        let mut edits = vec![underscore(l.span)];
        let mut certain = true;
        for e in &body.exprs {
            if !matches!(e.kind, hir::ExprKind::Local(x) if x.index() == i) {
                continue;
            }
            // Written by a macro, or otherwise not the name as it is in the file: no edit
            // here can be certain, so the rename is left to the help.
            if e.span.len() as usize != l.name.len() {
                certain = false;
                continue;
            }
            edits.push(underscore(e.span));
        }
        let d = d.with_help(format!(
            "if that is deliberate, name it `_{}`; otherwise remove it",
            l.name
        ));
        let d = if certain { d.with_fix(format!("rename it `_{}`", l.name), edits) } else { d };
        unread.pending.push((key, d));
    }
}

// ---------------------------------------------------------------------------
// places
// ---------------------------------------------------------------------------

/// A location a value can be moved out of: a local, or a field path from one.
///
/// The path is field *indices*, not names, because the HIR already resolved them — which
/// is what makes `a.b` and `a.c` distinguishable without consulting any declaration.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct Place {
    local: LocalId,
    path: Vec<usize>,
}

impl Place {
    /// Does moving `self` invalidate `other`, or the other way round?
    ///
    /// Moving `a` takes `a.b` with it, and moving `a.b` makes a later read of `a` a read
    /// of a partially-moved value. Both are prefix relationships, in opposite directions.
    fn overlaps(&self, other: &Place) -> bool {
        self.local == other.local && self.path.iter().zip(&other.path).all(|(a, b)| a == b)
    }

    fn is_prefix_of(&self, other: &Place) -> bool {
        self.local == other.local
            && self.path.len() <= other.path.len()
            && self.path.iter().zip(&other.path).all(|(a, b)| a == b)
    }
}

/// What an expression denotes, for the purpose of moving out of it.
enum Denotes {
    /// A place this function owns and can therefore reset to its defaults.
    Owned(Place),
    /// A place reached through a `&`, a `^`, or a dynamic index. The function does not
    /// own it, so nothing may be moved out of it (spec §6.4).
    Borrowed,
    /// A value with no home: a call result, a literal, an aggregate under construction.
    /// Moving one is a no-op, and copying one costs nothing anyone can observe.
    Temporary,
}

// ---------------------------------------------------------------------------

struct Flow<'a> {
    body: &'a hir::Body,
    table: &'a TypeTable,
    res: &'a Resolution,
    file: FileId,
    diags: &'a mut Diagnostics,
    /// Places moved so far, each with the span of the `move` that did it.
    moved: HashMap<Place, Span>,
    /// The moves nothing wrote whose value can be cloned: a read after one is fixed by
    /// cloning there instead, which is what the read meant before values moved by default
    /// (D-0106).
    cloneable: std::collections::HashSet<Span>,
    /// Set while replaying a loop body to find what it moves. The replay must not report,
    /// or every diagnostic inside a loop would appear twice.
    quiet: bool,
    /// The loops being walked, innermost last, each with what was moved at every `break`
    /// and `continue` that leaves or restarts it (T-0227).
    loops: Vec<LoopFrame>,
}

/// One loop being walked: its label's id, when a nested loop names it, and the moved set at
/// each jump to it. A jump diverges, so what it moved reaches no join in the body; it reaches
/// the loop instead, at the next pass or after the loop.
struct LoopFrame {
    id: Option<u32>,
    breaks: Vec<HashMap<Place, Span>>,
    continues: Vec<HashMap<Place, Span>>,
}

impl<'a> Flow<'a> {
    fn error(&mut self, d: Diagnostic) {
        if !self.quiet {
            self.diags.push(d);
        }
    }

    fn type_name(&self, t: &Ty) -> String {
        // A trait is a value only as `Self` in its own default bodies, where it was written
        // `Self` and means the implementer.
        if let Ty::Named { sym, .. } = t {
            if self.res.symbol(*sym).kind == dz_resolve::SymbolKind::Trait {
                return "Self".to_owned();
            }
        }
        dz_check::type_name(self.res, t)
    }

    fn body(&mut self) {
        let diverges = self.block(self.body.root);
        // Spec §7.2: a function that promises a value has to produce one on every path.
        // A fallible signature is included — `!-> void` still has to say it succeeded,
        // but codegen supplies that, so only a real return type is checked here.
        let wants_value = !matches!(self.body.ret, Ty::Void | Ty::Error | Ty::Never);
        if wants_value && !diverges && !self.body.blocks[self.body.root.index()].stmts.is_empty() {
            let ret = self.type_name(&self.body.ret);
            self.error(
                Diagnostic::error(
                    codes::MISSING_RETURN,
                    format!("this function must return `{ret}` and does not on every path"),
                )
                .at(self.file, self.body.span, "some path reaches the end without a value")
                .with_help(
                    "add a `return` at the end, or give the last `if` an `else` so every \
                     branch produces one",
                ),
            );
        }
    }

    // --- views of temporaries ----------------------------------------------

    /// Spec §6.5: a value that is kept — bound, assigned or returned — must not be a view
    /// into a temporary, which is dropped when the statement ends. Passing one to a call is
    /// fine, since the temporary outlives the call; keeping it is a read of freed memory
    /// once the statement is over, and nothing at run time says so.
    fn kept_view(&mut self, value: ExprId) {
        if !self.borrows(&self.body.ty(value).clone(), 0) {
            return;
        }
        let Some(owner) = self.viewed_temporary(value) else { return };
        let node = self.body.expr(owner);
        let ty = self.type_name(&node.ty);
        let span = node.span;
        self.error(
            Diagnostic::error(
                codes::VIEW_OF_TEMPORARY,
                format!(
                    "this keeps a view into a `{ty}` that is dropped at the end of the statement"
                ),
            )
            .at(self.file, span, "a temporary, gone once the statement ends")
            .with_help(format!(
                "bind the `{ty}` first so it lives on — `let owned = …`, then take the view \
                 from `owned` (spec §6.5)"
            )),
        );
    }

    /// Does a value of `ty` look into memory something else owns — a `str`, a slice, a
    /// reference or pointer, or a struct, tuple or optional holding one? A value that needs
    /// a drop owns what it holds, so it is no view: a `String` keeps a `^mut Allocator`,
    /// and `Path("a") / "b"` is a new `Path`, not a look into the old one.
    fn borrows(&self, ty: &Ty, depth: u32) -> bool {
        if depth > 8 || self.table.needs_drop(ty) {
            return false;
        }
        match ty {
            Ty::Str | Ty::CStr | Ty::Slice { .. } | Ty::Ref { .. } | Ty::Ptr { .. } => true,
            Ty::Optional(inner) | Ty::Array { elem: inner, .. } => self.borrows(inner, depth + 1),
            Ty::Tuple(parts) => parts.iter().any(|p| self.borrows(p, depth + 1)),
            Ty::Named { sym, args } => match self.table.struct_sig(*sym) {
                Some(sig) => self
                    .table
                    .field_types(*sym, args, &sig.fields)
                    .iter()
                    .any(|f| self.borrows(f, depth + 1)),
                None => false,
            },
            _ => false,
        }
    }

    /// The temporary a view computed by `e` looks into, following the chain a view is
    /// taken along — a method's receiver, a field, an element, a slice — to a value made
    /// in this statement that owns something. Not through a call's arguments: a function
    /// may return a view of anything, while a method on a temporary that hands back a view
    /// is handing back one of itself. A literal `&make()` is not followed either, since it
    /// lives to the end of its block (§14.2).
    fn viewed_temporary(&self, e: ExprId) -> Option<ExprId> {
        let next = match &self.body.expr(e).kind {
            hir::ExprKind::Call {
                callee: hir::Callee::Method { recv, .. } | hir::Callee::BaseMethod { recv, .. },
                ..
            } => *recv,
            hir::ExprKind::Field { base, .. }
            | hir::ExprKind::TupleIndex { base, .. }
            | hir::ExprKind::Index { base, .. }
            | hir::ExprKind::SliceRange { base, .. }
            | hir::ExprKind::DataPtr(base)
            | hir::ExprKind::Deref(base)
            | hir::ExprKind::Cast { operand: base, .. } => *base,
            _ => return None,
        };
        let node = self.body.expr(next);
        let made = matches!(
            node.kind,
            hir::ExprKind::Call { .. }
                | hir::ExprKind::StructLit { .. }
                | hir::ExprKind::ArrayLit(_)
                | hir::ExprKind::Tuple(_)
        );
        // Inside a generic body a `T` may or may not own anything; only a type known to
        // need a drop is known to free what the view looks at.
        if made && !node.ty.has_params() && self.table.needs_drop(&node.ty) {
            return Some(next);
        }
        self.viewed_temporary(next)
    }

    // --- statements --------------------------------------------------------

    /// Walk a block, returning whether every path through it diverges.
    fn block(&mut self, id: BlockId) -> bool {
        let stmts = self.body.block(id).stmts.clone();
        let mut diverges = false;
        for s in stmts {
            if self.stmt(s) {
                diverges = true;
            }
        }
        diverges
    }

    fn stmt(&mut self, id: StmtId) -> bool {
        let stmt = self.body.stmt(id);
        match &stmt.kind {
            hir::StmtKind::Let { local, init } => {
                if let Some(init) = init {
                    let diverges = self.value(*init);
                    self.kept_view(*init);
                    // A `let` rebinds, so a local that was moved and is now the target of
                    // a fresh binding is live again.
                    if let Some(l) = local {
                        self.revive(&Place { local: *l, path: Vec::new() });
                    }
                    return diverges;
                }
                if let Some(l) = local {
                    self.revive(&Place { local: *l, path: Vec::new() });
                }
                false
            }
            hir::StmtKind::LetTuple { locals, init } => {
                let diverges = self.value(*init);
                self.kept_view(*init);
                for l in locals.iter().flatten() {
                    self.revive(&Place { local: *l, path: Vec::new() });
                }
                diverges
            }
            hir::StmtKind::Assign { target, op, value, .. } => {
                let diverges = self.value(*value);
                // Plain assignment overwrites, so it re-initialises a moved-from place.
                // A compound assignment *reads* the target first, so it does not.
                if *op == dz_hir::AssignOp::Assign {
                    self.kept_view(*value);
                    if let Denotes::Owned(p) = self.denotes(*target) {
                        self.revive(&p);
                        return diverges;
                    }
                    self.place_only(*target);
                    return diverges;
                }
                self.read(*target) || diverges
            }
            hir::StmtKind::Expr(e) => self.value(*e),
            // A `loop` no `break` leaves ends only by `return`, so nothing after it runs and
            // a function may end with it (spec §10.2, D-0102). A `while` may always end.
            hir::StmtKind::While { cond, body, forever, label } => {
                self.read(*cond);
                self.loop_body(*body, &[], label.as_ref().map(|l| l.id));
                *forever
            }
            hir::StmtKind::For { bindings, subject, body, shape, label, .. } => {
                // A user iterator is copied or moved into the loop's own iterator, which
                // `advance` then mutates (spec §10.4) — so its subject is a value, which an
                // iterator that is not plain moves into. An array or slice is walked in place,
                // unless the loop consumes an array it owns (D-0106).
                if *shape == hir::ForShape::Consuming
                    || matches!(
                        self.body.ty(*subject).peeled(),
                        Ty::Named { .. } | Ty::Param(_) | Ty::Assoc(_)
                    )
                {
                    self.value(*subject);
                } else {
                    self.read(*subject);
                }
                let fresh: Vec<LocalId> = bindings.iter().flatten().copied().collect();
                self.loop_body(*body, &fresh, label.as_ref().map(|l| l.id));
                false
            }
            // A deferred statement runs at scope exit, among the drops in reverse order of
            // declaration (spec §6.5). What it reads is read; when it reads is not
            // something this analysis models.
            hir::StmtKind::Defer(inner) => {
                self.stmt(*inner);
                false
            }
            // Followed one call after another: whatever two calls would make of the same
            // place at once, a move in one beside a read in the other included, the ties
            // analysis refuses (spec §22.3), so the order cannot show.
            hir::StmtKind::Parallel { calls } => {
                let calls = calls.clone();
                let mut diverges = false;
                for c in calls {
                    diverges |= self.value(c);
                }
                diverges
            }
            hir::StmtKind::Item(_) | hir::StmtKind::Error => false,
        }
    }

    /// A loop body runs an unknown number of times, so anything it moves is already moved
    /// when the body starts again. Replaying it from `entry ∪ what-it-moves` is what turns
    /// "moved on the previous iteration" into a diagnostic at the use rather than silence.
    ///
    /// `fresh` are a `for`'s bindings, which every pass binds anew: moving the element out
    /// of `x` in `for x in it { take(move(x)) }` leaves nothing for the next pass to find
    /// missing, because the next pass has another `x`.
    fn loop_body(&mut self, body: BlockId, fresh: &[LocalId], id: Option<u32>) {
        let revive_fresh = |this: &mut Self| {
            for l in fresh {
                this.revive(&Place { local: *l, path: Vec::new() });
            }
        };
        let join = |into: &mut HashMap<Place, Span>, from: HashMap<Place, Span>| {
            for (place, span) in from {
                into.entry(place).or_insert(span);
            }
        };
        let frame = || LoopFrame { id, breaks: Vec::new(), continues: Vec::new() };
        revive_fresh(self);
        let entry = self.moved.clone();
        let was_quiet = std::mem::replace(&mut self.quiet, true);
        self.loops.push(frame());
        let diverges = self.block(body);
        let first = self.loops.pop().expect("pushed above");
        self.quiet = was_quiet;
        // A pass starts again from the body's end, when that is reachable, and from each
        // `continue`: not from a `return`, which leaves the function, nor a `break` (T-0227).
        let ended = std::mem::replace(&mut self.moved, entry);
        if !diverges {
            join(&mut self.moved, ended);
        }
        for state in first.continues {
            join(&mut self.moved, state);
        }
        let again = self.moved.clone();
        revive_fresh(self);
        self.loops.push(frame());
        let diverges = self.block(body);
        let second = self.loops.pop().expect("pushed above");
        // After the loop: where a pass begins, at which a `while` or `for` may stop, and
        // each `break`, whose moves reach what follows the loop.
        let ended = std::mem::replace(&mut self.moved, again);
        if !diverges {
            join(&mut self.moved, ended);
        }
        for state in second.continues.into_iter().chain(second.breaks) {
            join(&mut self.moved, state);
        }
    }

    // --- expressions -------------------------------------------------------

    /// The operand of `or`, or of a plain `if let` binding: a move-only optional held in a
    /// place the function owns is taken, leaving the place moved-from, as `move` would
    /// (spec §9.4). Anything else is an ordinary value.
    fn consumed(&mut self, e: ExprId) -> bool {
        let ty = self.body.ty(e).clone();
        if matches!(ty, Ty::Optional(_)) && !self.table.plain(&ty) && !self.body.is_clone(e) {
            if let Denotes::Owned(p) = self.denotes(e) {
                let span = self.body.expr(e).span;
                self.check_live(&p, span);
                self.walk_place_bases(e);
                self.moved.insert(p, span);
                return false;
            }
        }
        self.value(e)
    }

    /// An expression whose *value* is wanted. A value that is not plain data moves here
    /// (D-0106): the checker wrote a `Move` around every such read of a place the function
    /// owns, so what is left is a read written `a.clone()`, a temporary, or a place the
    /// function cannot move out of.
    fn value(&mut self, e: ExprId) -> bool {
        let ty = self.body.ty(e).clone();
        let moves = !self.table.plain(&ty)
            && !self.body.is_clone(e)
            && !matches!(self.body.expr(e).kind, hir::ExprKind::Move(_));
        if moves {
            match self.denotes(e) {
                // Already moved: `read` below reports that, and it is the more useful of
                // the two answers — saying "this would copy it" about a value that is not
                // there any more only buries the real mistake.
                Denotes::Owned(p) if self.moved.keys().any(|q| q.overlaps(&p)) => {}
                // The checker moves every such read (`dz_check::moves`), so one left here is
                // a context the two disagree about: reported, rather than copied silently.
                Denotes::Owned(_) => {
                    let name = self.type_name(&ty);
                    let why = self
                        .table
                        .move_only_because(&ty)
                        .unwrap_or_else(|| "it is not plain data".to_owned());
                    let span = self.body.expr(e).span;
                    let d = Diagnostic::error(
                        codes::IMPLICIT_COPY,
                        format!("`{name}` would be copied here, and is not plain data"),
                    )
                    .at(self.file, span, "this would copy it");
                    let d = self.clone_fix(d, &ty, span, why);
                    self.error(d);
                }
                Denotes::Borrowed => {
                    let name = self.type_name(&ty);
                    let span = self.body.expr(e).span;
                    let (from, label) = if self.in_static(e) {
                        ("a `static`", "this belongs to the program, not to this function")
                    } else {
                        ("a reference", "this is borrowed, not owned")
                    };
                    let d = Diagnostic::error(
                        codes::MOVE_OUT_OF_BORROW,
                        format!("`{name}` cannot be moved out of {from}"),
                    )
                    .at(self.file, span, label);
                    let why = "a value is moved only out of something the function owns, since \
                               the source is reset to its defaults (spec §6.4)"
                        .to_owned();
                    let d = self.clone_fix(d, &ty, span, why);
                    self.error(d);
                }
                Denotes::Temporary => {}
            }
        }
        self.read(e)
    }

    /// `help:` saying why, and for a type that can be cloned, the fix that clones it: a
    /// value read where it may not be moved is a copy, and `.clone()` is how a copy is
    /// written (D-0106). Certain, since it is what the read meant before values moved by
    /// default, and `dz fix` migrates a program by it.
    fn clone_fix(&self, d: Diagnostic, ty: &Ty, span: Span, why: String) -> Diagnostic {
        if !self.table.copyable(ty) {
            return d.with_help(why);
        }
        let edit = dz_diag::Edit {
            file: self.file,
            span: Span { start: span.end, end: span.end },
            text: ".clone()".to_owned(),
        };
        d.with_help(format!("{why}; write `.clone()` for a copy"))
            .with_fix("write `.clone()`", vec![edit])
    }

    /// An expression evaluated for its effects and its sub-reads, but not copied: the base
    /// of a `.`, the operand of `&`, a method receiver.
    fn place_only(&mut self, e: ExprId) -> bool {
        self.read(e)
    }

    /// Walk what a place expression is *built from*, without checking the place itself.
    ///
    /// `a.b[i].c` reads `i`, and the caller has already asked about `a.b[i].c` as a whole,
    /// so this exists to visit the parts without asking twice.
    fn walk_place_bases(&mut self, e: ExprId) -> bool {
        match self.body.expr(e).kind {
            hir::ExprKind::Field { base, .. } | hir::ExprKind::TupleIndex { base, .. } => {
                self.walk_place_bases(base)
            }
            hir::ExprKind::Index { base, index } => {
                self.place_only(base);
                self.value(index);
                false
            }
            hir::ExprKind::Deref(inner) => self.place_only(inner),
            _ => false,
        }
    }

    /// Walk an expression, checking every place it reads and recording every move it
    /// performs. Returns whether it always diverges.
    fn read(&mut self, e: ExprId) -> bool {
        let node = self.body.expr(e);
        let span = node.span;
        match node.kind.clone() {
            hir::ExprKind::Local(_)
            | hir::ExprKind::Field { .. }
            | hir::ExprKind::TupleIndex { .. } => {
                if let Denotes::Owned(p) = self.denotes(e) {
                    self.check_live(&p, span);
                }
                self.walk_place_bases(e)
            }
            hir::ExprKind::Index { base, index } => {
                self.place_only(base);
                self.value(index);
                false
            }
            hir::ExprKind::Deref(inner) => self.place_only(inner),
            hir::ExprKind::Ref { place, .. } => self.place_only(place),

            hir::ExprKind::Move(inner) => {
                let ty = self.body.ty(inner).clone();
                match self.denotes(inner) {
                    Denotes::Owned(p) => {
                        // Checked against the `move(…)` node's span, not the operand's,
                        // and the operand is then walked for its *bases* only — checking
                        // it again would report the same mistake at two columns of the
                        // same expression (invariant 6 in CLAUDE.md).
                        self.check_live(&p, span);
                        self.walk_place_bases(inner);
                        // A move ends its source whatever the type: a copyable value is
                        // moved-from afterwards like any other (spec §6.4, §18.33).
                        self.moved.insert(p, span);
                        // One the checker wrote has its operand's span (`dz_check::moves`).
                        if self.body.expr(inner).span == span && self.table.copyable(&ty) {
                            self.cloneable.insert(span);
                        }
                    }
                    Denotes::Borrowed => {
                        let name = self.type_name(&ty);
                        let (from, label) = if self.in_static(inner) {
                            ("a `static`", "this belongs to the program, not to this function")
                        } else {
                            ("a reference", "this is borrowed, not owned")
                        };
                        self.error(
                            Diagnostic::error(
                                codes::MOVE_OUT_OF_BORROW,
                                format!("cannot move `{name}` out of {from}"),
                            )
                            .at(self.file, span, label)
                            .with_help(
                                "the source of a move is reset to its defaults, and a borrow \
                                 does not give the right to do that (spec §6.4); to copy the \
                                 value instead, write `.clone()`",
                            ),
                        );
                        self.place_only(inner);
                    }
                    Denotes::Temporary => {
                        self.value(inner);
                    }
                }
                false
            }

            hir::ExprKind::Unary { operand, .. } => self.value(operand),
            hir::ExprKind::Binary { lhs, rhs, .. } => {
                let a = self.value(lhs);
                let b = self.value(rhs);
                a && b
            }
            hir::ExprKind::Cast { operand, .. } => self.value(operand),
            hir::ExprKind::Range { start, end, .. } => {
                for x in [start, end].into_iter().flatten() {
                    self.value(x);
                }
                false
            }

            hir::ExprKind::StructLit { fields, .. } => {
                for f in fields.into_iter().flatten() {
                    self.value(f);
                }
                false
            }
            hir::ExprKind::UnionVariant { payload, .. } => {
                for f in payload.into_iter().flatten() {
                    self.value(f);
                }
                false
            }
            hir::ExprKind::Tuple(parts) | hir::ExprKind::ArrayLit(parts) => {
                for p in parts {
                    self.value(p);
                }
                false
            }
            // A pack's tuple passed on as its elements, each taken (spec §8.7).
            hir::ExprKind::Spread { base: inner, .. } => self.value(inner),
            // A pattern over a pack: its tuple taken when the pattern takes the element,
            // which the moves pass wrote as a move of the tuple, and read otherwise.
            hir::ExprKind::Expand { over, pattern, .. } => {
                if matches!(self.body.expr(over).kind, hir::ExprKind::Move(_)) {
                    self.value(over);
                } else {
                    self.place_only(over);
                }
                self.value(pattern)
            }
            hir::ExprKind::Expanded(copies) => {
                for c in copies {
                    self.value(c);
                }
                false
            }

            hir::ExprKind::Call { callee, args } => {
                // A base-qualified call borrows its receiver exactly as `recv.m()` does
                // (spec §7.3, 18.32).
                if let hir::Callee::Method { recv, .. } | hir::Callee::BaseMethod { recv, .. } =
                    &callee
                {
                    self.place_only(*recv);
                } else if let hir::Callee::Value(v) = &callee {
                    self.value(*v);
                }
                for a in args {
                    self.value(a);
                }
                false
            }
            // Asking a container its length reads nothing out of it, so the container is a
            // place here — `.len` on an array of move-only elements is not a copy.
            hir::ExprKind::Len(base) | hir::ExprKind::DataPtr(base) => self.place_only(base),
            // `construct(dst, …)` and `destroy(dst)` act on `dst` in place (spec §14.3): it is
            // storage being initialised or torn down, not a value being read, so it is
            // neither copied nor moved out of — the same as a method receiver. Treating it as
            // a value made `destroy(m_data[i])` a "move out of a borrow".
            hir::ExprKind::Builtin { name, args }
                if matches!(name.as_str(), "construct" | "destroy" | "move_out") =>
            {
                let mut rest = args.into_iter();
                if let Some(dst) = rest.next() {
                    self.place_only(dst);
                }
                for a in rest {
                    self.value(a);
                }
                false
            }
            hir::ExprKind::Builtin { args, .. } | hir::ExprKind::Intrinsic { args, .. } => {
                for a in args {
                    self.value(a);
                }
                false
            }

            hir::ExprKind::Block(b) => self.block(b),
            hir::ExprKind::If { cond, then, else_branch } => {
                self.value(cond);
                // An `if` with no `else` still has the path where nothing happens, and
                // that path reaches whatever follows — so it is a branch, not an absence.
                let other = else_branch.map(Branch::Block).unwrap_or(Branch::Nothing);
                self.branches(&[Branch::Block(then)], Some(other))
            }
            // The operand is read as `or` reads it; then the two blocks are the branches.
            // The guard is initialised like a `let`; the pointer is then read from it.
            hir::ExprKind::Guarded { guard, init, value } => {
                let diverges = self.value(init);
                self.revive(&Place { local: guard, path: Vec::new() });
                self.value(value) || diverges
            }
            // A formatting macro's arguments are a call's (spec §12.9): each is read and
            // bound, then the body writes them.
            hir::ExprKind::Formatted { args, params, body } => {
                let mut diverges = false;
                for a in args {
                    diverges |= self.value(a);
                }
                for p in params {
                    self.revive(&Place { local: p, path: Vec::new() });
                }
                self.block(body) || diverges
            }
            // A reference binding (`&v`, `&mut v`) refers into the operand, so it is only
            // read in place; a plain one takes the value out, as `or` does (18.61).
            hir::ExprKind::IfLet { binding, operand, then, else_branch } => {
                if matches!(self.body.local(binding).ty, Ty::Ref { .. }) {
                    self.place_only(operand);
                } else {
                    self.consumed(operand);
                }
                let other = else_branch.map(Branch::Block).unwrap_or(Branch::Nothing);
                self.branches(&[Branch::Block(then)], Some(other))
            }
            hir::ExprKind::Switch { subject, arms, default } => {
                self.place_only(subject);
                if arms.is_empty() && default.is_none() {
                    return false;
                }
                let branches: Vec<Branch> = arms.iter().map(|a| Branch::Block(a.body)).collect();
                // Without a `default` the checker proved the arms exhaustive (DZ0513), so
                // there is no fall-through path and "every arm diverges" really does mean
                // the whole switch does.
                self.branches(&branches, default.map(Branch::Block))
            }
            hir::ExprKind::Or { operand, clause } => {
                self.consumed(operand);
                let fail = match &clause {
                    hir::OrClause::Value(v) => Branch::Expr(*v),
                    hir::OrClause::Block { body, .. } => Branch::Block(*body),
                    // Neither produces a value, and neither diverges.
                    hir::OrClause::Ignore | hir::OrClause::Unreachable => Branch::Nothing,
                };
                // The success path does nothing observable; only the clause can move or
                // diverge, so the join is "clause ran" against "it did not".
                self.branches(&[Branch::Nothing], Some(fail))
            }

            hir::ExprKind::Return(v) => {
                if let Some(v) = v {
                    // `return x` of a whole local is a move C++ already makes, and the
                    // checker leaves it unwritten so the value can be built in place
                    // (`dz_check::moves`): nothing after it can see the local again. Not
                    // `self`, a receiver the method borrows: `return self` of a value that
                    // is not plain would copy it (T-0241).
                    if matches!(self.body.expr(v).kind, hir::ExprKind::Local(_))
                        && matches!(self.denotes(v), Denotes::Owned(_))
                    {
                        self.read(v);
                    } else {
                        self.value(v);
                    }
                    self.kept_view(v);
                }
                true
            }
            // A jump diverges: what it moved goes to the loop it names, the innermost or the
            // one its label's id picks, at the next pass or after the loop (T-0227).
            hir::ExprKind::Break(target) | hir::ExprKind::Continue(target) => {
                let at = match target {
                    None => self.loops.len().checked_sub(1),
                    Some(id) => self.loops.iter().rposition(|f| f.id == Some(id)),
                };
                if let Some(i) = at {
                    let state = self.moved.clone();
                    let frame = &mut self.loops[i];
                    if matches!(self.body.expr(e).kind, hir::ExprKind::Break(_)) {
                        frame.breaks.push(state);
                    } else {
                        frame.continues.push(state);
                    }
                }
                true
            }
            // A slice views its base in place; the ends are values (spec §5.3).
            hir::ExprKind::SliceRange { base, start, end } => {
                self.place_only(base);
                for x in [start, end].into_iter().flatten() {
                    self.value(x);
                }
                false
            }
            // `panic!(...)` never comes back, which is what its `Never` type says.
            _ => matches!(self.body.ty(e), Ty::Never),
        }
    }

    /// Analyse alternative paths from one state and merge the results.
    ///
    /// The merge is a union: a place moved on any path is moved after the join, because
    /// §6.4 makes "moved on some paths reaching here" an error at the use.
    fn branches(&mut self, arms: &[Branch], default: Option<Branch>) -> bool {
        let entry = self.moved.clone();
        let mut merged: HashMap<Place, Span> = HashMap::new();
        // Every listed path has to diverge for the whole construct to. A caller that
        // passes no default is saying the paths given are all of them.
        let mut all_diverge = true;

        let run = |cx: &mut Self, b: &Branch| -> bool {
            cx.moved = entry.clone();
            match b {
                Branch::Block(id) => cx.block(*id),
                Branch::Expr(e) => cx.value(*e),
                Branch::Nothing => false,
            }
        };

        for b in arms {
            let diverges = run(self, b);
            if !diverges {
                // A path that diverges never reaches the join, so what it moved cannot
                // be observed after it.
                for (p, s) in self.moved.clone() {
                    merged.entry(p).or_insert(s);
                }
            }
            all_diverge &= diverges;
        }
        if let Some(b) = &default {
            let diverges = run(self, b);
            if !diverges {
                for (p, s) in self.moved.clone() {
                    merged.entry(p).or_insert(s);
                }
            }
            all_diverge &= diverges;
        }

        self.moved = if merged.is_empty() { entry } else { merged };
        all_diverge
    }

    // --- the moved set -----------------------------------------------------

    fn revive(&mut self, p: &Place) {
        self.moved.retain(|q, _| !p.is_prefix_of(q) && !q.is_prefix_of(p));
    }

    /// Report if `p`, anything containing it, or anything inside it has been moved.
    fn check_live(&mut self, p: &Place, span: Span) {
        let hit = self.moved.iter().find(|(q, _)| q.overlaps(p)).map(|(q, s)| (q.clone(), *s));
        let Some((q, at)) = hit else { return };
        let name = self.body.local(p.local).name.clone();
        let whole = q.is_prefix_of(p);
        // The move and the use being the same expression means the loop carried it round:
        // the value was moved on the previous iteration, not before the loop.
        let looped = at.start <= span.start && span.end <= at.end;
        let d = if whole {
            Diagnostic::error(
                codes::USE_AFTER_MOVE,
                format!("`{name}` was moved and cannot be used again"),
            )
            .at(
                self.file,
                span,
                if looped {
                    "this runs more than once, and the second time it finds nothing here"
                } else {
                    "used here after the move"
                },
            )
            .with_secondary(self.file, at, "moved here")
            .with_help(if self.cloneable.contains(&at) {
                "a value that is not plain data moves when it is used by value, and a \
                 moved-from value is left at its defaults (spec §6.4); to keep this one, \
                 move a copy of it: write `.clone()` where it moved"
            } else {
                "a moved-from value is left at its defaults, and reading it back would \
                 almost never be what was meant (spec §6.4)"
            })
        } else {
            let field = self.field_path(&q);
            Diagnostic::error(
                codes::USE_OF_PARTIAL_MOVE,
                format!("`{name}` is only partly there: `{field}` was moved out of it"),
            )
            .at(self.file, span, "the whole value is used here")
            .with_secondary(self.file, at, "this field was moved out")
            .with_help(
                "move the remaining fields out one at a time, or move the whole value \
                 before taking any field out of it (spec §6.4)",
            )
        };
        // A move nothing wrote, of a value that can be cloned, is what was a copy before
        // values moved by default; writing `.clone()` there keeps it one (D-0106).
        let d = if self.cloneable.contains(&at) {
            let edit = dz_diag::Edit {
                file: self.file,
                span: Span { start: at.end, end: at.end },
                text: ".clone()".to_owned(),
            };
            d.with_fix("clone it where it moved: write `.clone()`", vec![edit])
        } else {
            d
        };
        self.error(d);
    }

    /// `a.b.c`, for a diagnostic that names which field went missing.
    fn field_path(&self, p: &Place) -> String {
        let mut out = self.body.local(p.local).name.clone();
        let mut ty = self.body.local(p.local).ty.clone();
        for index in &p.path {
            let name = match ty.peeled() {
                Ty::Named { sym, .. } => self
                    .table
                    .struct_sig(*sym)
                    .and_then(|s| s.fields.get(*index))
                    .map(|f| (f.name.clone(), f.ty.clone())),
                _ => None,
            };
            match name {
                Some((n, t)) => {
                    out.push('.');
                    out.push_str(&n);
                    ty = t;
                }
                None => {
                    out.push_str(&format!(".{index}"));
                    ty = Ty::Error;
                }
            }
        }
        out
    }

    /// What an expression denotes: an owned place, a borrow, or a temporary.
    fn denotes(&self, e: ExprId) -> Denotes {
        match &self.body.expr(e).kind {
            hir::ExprKind::Local(l) => {
                // A method's `self` is a receiver, not something the method owns: §7.3
                // gives methods `&self`/`&mut self`, and the HIR records the struct type
                // because `.` reaches through either way.
                if self.body.local(*l).name == "self" {
                    Denotes::Borrowed
                } else {
                    Denotes::Owned(Place { local: *l, path: Vec::new() })
                }
            }
            hir::ExprKind::Field { base, index } | hir::ExprKind::TupleIndex { base, index } => {
                // Reaching through a pointer or reference lands outside this function.
                if matches!(
                    self.body.ty(*base),
                    Ty::Ptr { .. } | Ty::OwnPtr { .. } | Ty::Ref { .. }
                ) {
                    return Denotes::Borrowed;
                }
                match self.denotes(*base) {
                    Denotes::Owned(mut p) => {
                        p.path.push(*index);
                        Denotes::Owned(p)
                    }
                    other => other,
                }
            }
            // An index is a runtime value, so which element moved is not knowable here.
            hir::ExprKind::Index { .. } | hir::ExprKind::Deref(_) => Denotes::Borrowed,
            // A `static` belongs to the whole program, not to this function, so nothing may
            // be moved or copied out of it any more than out of a borrow (spec §7.7).
            hir::ExprKind::Static(_) => Denotes::Borrowed,
            _ => Denotes::Temporary,
        }
    }

    /// Is this place part of a `static`, reached without going through a pointer? Only for
    /// wording: the rules are a borrow's.
    fn in_static(&self, e: ExprId) -> bool {
        match self.body.expr(e).kind {
            hir::ExprKind::Static(_) => true,
            hir::ExprKind::Field { base, .. } | hir::ExprKind::TupleIndex { base, .. } => {
                !matches!(self.body.ty(base), Ty::Ptr { .. } | Ty::OwnPtr { .. } | Ty::Ref { .. })
                    && self.in_static(base)
            }
            _ => false,
        }
    }
}

/// One alternative path through an `if`, a `switch` arm or an `or` clause.
#[derive(Clone, Copy)]
enum Branch {
    Block(BlockId),
    Expr(ExprId),
    /// The path where nothing happens: an `if` with no `else`, an `or` that succeeded.
    Nothing,
}
