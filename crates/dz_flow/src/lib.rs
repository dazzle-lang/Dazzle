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
use dz_span::{FileId, Span};

pub mod ties;

/// Check every body in the program.
pub fn check(hir: &Hir, table: &TypeTable, res: &Resolution) -> Diagnostics {
    let mut diags = Diagnostics::new();
    // The prelude is the compiler's own code: a warning there is noise in every program.
    let prelude: std::collections::HashSet<FileId> =
        res.symbols.iter().filter(|s| s.module == res.prelude).map(|s| s.file).collect();
    let mut unread = Unread::default();
    for (_, body) in hir.bodies() {
        let mut cx = Flow {
            body,
            table,
            res,
            file: body.file,
            diags: &mut diags,
            moved: HashMap::new(),
            quiet: false,
        };
        cx.body();
        // References used after their memory was killed, and clauses that do not say what
        // the body kills (spec §6.7).
        let analysis = ties::analyse_body(body, table, res);
        ties::report(body, table, res, &analysis, &mut diags);
        // A trait method's parameters are declarations, not bindings anything could read:
        // its body exists only to hold their defaults (spec §7.6).
        let signature_only = matches!(body.owner, dz_hir::BodyOwner::TraitDefaults { .. });
        if !prelude.contains(&body.file) && !signature_only {
            unused(body, &mut unread);
        }
    }
    unread.report(&mut diags);
    diags.sort_by_position();
    diags
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
    for (l, u) in body.locals.iter().zip(hir::local_uses(body)) {
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
        // is the whole of the rename: a leading `_` is what says "unused on purpose".
        let underscore = dz_diag::Edit {
            file: body.file,
            span: dz_span::Span { start: l.span.start, end: l.span.start },
            text: "_".to_owned(),
        };
        unread.pending.push((
            key,
            d.with_help(format!(
                "if that is deliberate, name it `_{}`; otherwise remove it",
                l.name
            ))
            .with_fix(format!("rename it `_{}`", l.name), vec![underscore]),
        ));
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
    /// Set while replaying a loop body to find what it moves. The replay must not report,
    /// or every diagnostic inside a loop would appear twice.
    quiet: bool,
}

impl<'a> Flow<'a> {
    fn copyable(&self, t: &Ty) -> bool {
        self.table.copyable(t)
    }

    fn error(&mut self, d: Diagnostic) {
        if !self.quiet {
            self.diags.push(d);
        }
    }

    fn type_name(&self, t: &Ty) -> String {
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
            hir::StmtKind::Assign { target, op, value } => {
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
            hir::StmtKind::While { cond, body, .. } => {
                self.read(*cond);
                self.loop_body(*body, &[]);
                false
            }
            hir::StmtKind::For { bindings, subject, body, shape, .. } => {
                // A user iterator is copied or moved into the loop's own iterator, which
                // `advance` then mutates (spec §10.4) — so its subject is a value, and a
                // move-only iterator needs a `move`. An array or slice is walked in place,
                // unless `for e in move(xs)` consumes it.
                if *shape == hir::ForShape::Consuming
                    || matches!(self.body.ty(*subject).peeled(), hir::ty::Ty::Named { .. })
                {
                    self.value(*subject);
                } else {
                    self.read(*subject);
                }
                let fresh: Vec<LocalId> = bindings.iter().flatten().copied().collect();
                self.loop_body(*body, &fresh);
                false
            }
            // A deferred statement runs at scope exit, after the drops (spec §6.5). What
            // it reads is read; when it reads is not something this analysis models.
            hir::StmtKind::Defer(inner) => {
                self.stmt(*inner);
                false
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
    fn loop_body(&mut self, body: BlockId, fresh: &[LocalId]) {
        let revive_fresh = |this: &mut Self| {
            for l in fresh {
                this.revive(&Place { local: *l, path: Vec::new() });
            }
        };
        revive_fresh(self);
        let entry = self.moved.clone();
        let was_quiet = std::mem::replace(&mut self.quiet, true);
        self.block(body);
        self.quiet = was_quiet;
        let after = std::mem::replace(&mut self.moved, entry);
        for (place, span) in after {
            self.moved.entry(place).or_insert(span);
        }
        revive_fresh(self);
        self.block(body);
    }

    // --- expressions -------------------------------------------------------

    /// The operand of `or`, or of a plain `if let` binding: a move-only optional held in a
    /// place the function owns is taken, leaving the place moved-from, as `move` would
    /// (spec §9.4). Anything else is an ordinary value.
    fn consumed(&mut self, e: ExprId) -> bool {
        let ty = self.body.ty(e).clone();
        if matches!(ty, Ty::Optional(_)) && !self.copyable(&ty) {
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

    /// An expression whose *value* is wanted. Reading a move-only place here is the copy
    /// spec §6.3 forbids.
    fn value(&mut self, e: ExprId) -> bool {
        let ty = self.body.ty(e).clone();
        if !self.copyable(&ty) && !matches!(self.body.expr(e).kind, hir::ExprKind::Move(_)) {
            match self.denotes(e) {
                // Already moved: `read` below reports that, and it is the more useful of
                // the two answers — saying "this would copy it" about a value that is not
                // there any more only buries the real mistake.
                Denotes::Owned(p) if self.moved.keys().any(|q| q.overlaps(&p)) => {}
                Denotes::Owned(_) => {
                    let name = self.type_name(&ty);
                    let why = self
                        .table
                        .move_only_because(&ty)
                        .unwrap_or_else(|| "it owns a resource".to_owned());
                    let span = self.body.expr(e).span;
                    // A generic parameter or a projection of one has no declaration of its
                    // own to give a `copy` to; its bound is what says it copies.
                    let help = if dz_hir::ty::is_abstract(&ty) {
                        format!("{why} — write `move(…)` to transfer ownership")
                    } else {
                        format!(
                            "{why} — write `move(…)` to transfer ownership, or give \
                             `{name}` a `copy(other: &Self)` to make it copyable (spec §6.3)"
                        )
                    };
                    self.error(
                        Diagnostic::error(
                            codes::IMPLICIT_COPY,
                            format!("`{name}` is move-only and cannot be copied"),
                        )
                        .at(self.file, span, "this would copy it")
                        .with_help(help),
                    );
                }
                Denotes::Borrowed => {
                    let name = self.type_name(&ty);
                    let span = self.body.expr(e).span;
                    let (from, label) = if self.in_static(e) {
                        ("a `static`", "this belongs to the program, not to this function")
                    } else {
                        ("a reference", "this is borrowed, not owned")
                    };
                    self.error(
                        Diagnostic::error(
                            codes::MOVE_OUT_OF_BORROW,
                            format!("`{name}` cannot be taken out of {from}"),
                        )
                        .at(self.file, span, label)
                        .with_help(
                            "a move-only value can only be moved out of something the \
                             function owns, since the source has to be reset to its \
                             defaults (spec §6.4)",
                        ),
                    );
                }
                Denotes::Temporary => {}
            }
        }
        self.read(e)
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
                                 value instead, leave out `move`",
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
                    self.value(v);
                    self.kept_view(v);
                }
                true
            }
            hir::ExprKind::Break(_) | hir::ExprKind::Continue(_) => true,
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
                    "this runs more than once, and the second time `move` finds nothing here"
                } else {
                    "used here after the move"
                },
            )
            .with_secondary(self.file, at, "moved here")
            .with_help(
                "a moved-from value is left at its defaults, and reading it back would \
                 almost never be what was meant (spec §6.4)",
            )
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
