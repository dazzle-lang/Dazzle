//! `parallel for`: a loop whose passes run at the same time (spec §22.3).
//!
//! Each pass is given an element no other pass is given, which the iterator promises
//! (`ParallelIterate`). Everything else a pass uses from outside the loop is shared by every
//! pass at once, so it may only be read, and only if it is `Share`; nothing may leave the loop
//! from inside a pass. The checker holds the body to that here: what is written is seen where
//! the checker already asks whether a place may be written, what is read and what is lent on
//! as `&mut` by a walk over the nodes the body built, and the jumps where they resolve.

use std::collections::HashSet;

use dz_ast::{Expr, ExprKind};
use dz_diag::{codes, Diagnostic};
use dz_hir::{self as hir, ExprId, LocalId};
use dz_span::Span;

use super::{Body, Slot};
use crate::threads::Crossing;
use crate::ty::Ty;
use crate::TraitRef;

/// One `parallel for` being checked: the first local and node its body made, so that
/// anything before them is the loop's outside.
#[derive(Clone, Copy, Debug)]
pub(super) struct ParallelLoop {
    pub(super) first_local: usize,
    pub(super) first_expr: usize,
}

impl Body<'_, '_> {
    /// Whether the loop's subject, as `for` iterates it, can be split between threads: a
    /// range, an array, a slice, or an iterator that implements `ParallelIterate`.
    pub(super) fn parallel_subject(&mut self, span: Span, subject: ExprId) {
        let ty = self.hir.ty(subject).clone();
        let ty = self.subst(&ty);
        if ty.is_wildcard() || matches!(self.hir.kind(subject), hir::ExprKind::Range { .. }) {
            return;
        }
        let splits = match ty.peeled_refs() {
            Ty::Range(_) | Ty::Array { .. } | Ty::Slice { .. } | Ty::ConstSlice(_) => true,
            t => self.cx.prelude_trait("ParallelIterate").is_some_and(|trait_sym| {
                let r = TraitRef { trait_sym, args: Vec::new(), span: Span::DUMMY };
                self.cx.table.implements(t, &r)
            }),
        };
        if !splits {
            let name = self.cx.name_of(&ty);
            self.cx.error_help(
                codes::PARALLEL_LOOP_SUBJECT,
                span,
                format!("`parallel for` cannot split `{name}` between threads"),
                format!("`{name}` is not `ParallelIterate`"),
                "a `parallel for` loops over a range, an array, a slice, or a container whose \
                 iterator implements `ParallelIterate`, as a `List`'s does (spec §22.3)",
            );
        }
    }

    /// Hold each binding to what may cross to the thread its pass runs on: an element lent
    /// as `&mut` or copied is `Send`, one lent as `&` is `Share` (spec §22.2).
    pub(super) fn parallel_elements(&mut self, bindings: &[Option<LocalId>], span: Span) {
        for l in bindings.iter().flatten() {
            let ty = self.hir.local_ty(*l).clone();
            let (which, inner) = match &ty {
                Ty::Ref { mutable: false, inner } => (Crossing::Share, (**inner).clone()),
                Ty::Ref { mutable: true, inner } => (Crossing::Send, (**inner).clone()),
                t => (Crossing::Send, t.clone()),
            };
            self.pass_crossing(span, &inner, which, "each pass is given", "");
        }
    }

    /// `e` is about to be written: an error if it is a place outside the `parallel for`
    /// being checked, which every pass shares (spec §22.3).
    pub(super) fn parallel_write(&mut self, e: &Expr) {
        let Some(pl) = self.parallel_loops.last().copied() else { return };
        let Some(name) = self.outer_root(e, pl) else { return };
        self.outer_written(e.span, &name, "written");
    }

    fn outer_written(&mut self, span: Span, name: &str, how: &str) {
        let already = self.cx.diags.iter().any(|d| {
            d.code == Some(codes::PARALLEL_LOOP_WRITES) && d.labels.iter().any(|l| l.span == span)
        });
        if already {
            return;
        }
        let d = Diagnostic::error(
            codes::PARALLEL_LOOP_WRITES,
            format!("a pass of `parallel for` writes `{name}`, which every pass shares"),
        )
        .at(self.cx.file, span, format!("{how} by every pass at once"))
        .with_help(
            "the passes run at the same time, so each writes only the element it is given; \
             change what the loop needs before or after it, or keep it in the element \
             (spec §22.3)",
        );
        self.cx.diags.push(d);
    }

    /// The name of the local `e` is a place in, when that local is from outside the loop
    /// `pl`: `w` for `w.positions[i].x`. A field of `self` named unqualified is `self`'s.
    fn outer_root(&self, e: &Expr, pl: ParallelLoop) -> Option<String> {
        match &e.kind {
            ExprKind::Path(p) if p.segments.len() == 1 => {
                let name = &p.segments[0].name.name;
                match self.local(name)?.slot {
                    Slot::Local(id) if id.index() < pl.first_local => Some(name.clone()),
                    Slot::SelfField { .. } => Some(name.clone()),
                    _ => None,
                }
            }
            ExprKind::Field { base, .. } | ExprKind::Index { base, .. } => {
                self.outer_root(base, pl)
            }
            ExprKind::Deref(inner) => self.outer_root(inner, pl),
            _ => None,
        }
    }

    /// After the body: what it reads from outside the loop is `Share`, nothing it reaches
    /// from outside is lent on as `&mut`, by being passed or bound, and, where the passes
    /// write their elements, nothing reaches the container they are in, `written`, but through
    /// the element each pass is given (spec §22.3).
    pub(super) fn parallel_body(
        &mut self,
        pl: ParallelLoop,
        written: Option<(LocalId, Vec<usize>)>,
    ) {
        let end = self.hir.expr_count();
        let ids = || (pl.first_expr..end).map(|i| ExprId(i as u32));
        if let Some(container) = written {
            self.reads_written(pl, &container);
        }
        // The nodes only read through: a field's base, an index's, a method's receiver (whose
        // `&mut self` the checker saw as a write already), what `&` refers to.
        let mut read_through = HashSet::new();
        for e in ids() {
            match self.hir.kind(e) {
                hir::ExprKind::Field { base, .. }
                | hir::ExprKind::TupleIndex { base, .. }
                | hir::ExprKind::Index { base, .. }
                | hir::ExprKind::Len(base)
                | hir::ExprKind::Deref(base)
                | hir::ExprKind::Ref { place: base, .. } => {
                    read_through.insert(*base);
                }
                hir::ExprKind::Call {
                    callee: hir::Callee::Method { recv, .. } | hir::Callee::BaseMethod { recv, .. },
                    ..
                } => {
                    read_through.insert(*recv);
                }
                _ => {}
            }
        }
        let mut read = HashSet::new();
        for e in ids() {
            let Some(root) = self.hir_root(e) else { continue };
            if root.index() >= pl.first_local {
                continue;
            }
            let name = self.hir.local_name(root).to_owned();
            if matches!(self.hir.kind(e), hir::ExprKind::Local(_)) && read.insert(root) {
                let ty = self.hir.local_ty(root).clone();
                let inner = match &ty {
                    Ty::Ref { inner, .. } => (**inner).clone(),
                    Ty::Slice { elem, .. } => (**elem).clone(),
                    t => t.clone(),
                };
                let span = self.hir.span(e);
                self.pass_crossing(span, &inner, Crossing::Share, "every pass reads", &name);
            }
            let lends = matches!(
                self.hir.ty(e),
                Ty::Ref { mutable: true, .. } | Ty::Slice { mutable: true, .. }
            );
            if lends
                && !read_through.contains(&e)
                && !matches!(self.hir.kind(e), hir::ExprKind::Ref { .. })
            {
                let span = self.hir.span(e);
                self.outer_written(span, &name, "lent as `&mut`");
            }
        }
    }

    /// Each place the body reaches from outside the loop that overlaps `container`, the one
    /// whose elements the passes write: another pass may be writing what it reads. A place is
    /// a local and the fields under it, so `self.scale` is apart from `self.speeds`; an array's
    /// length is never written and may be read.
    fn reads_written(&mut self, pl: ParallelLoop, container: &(LocalId, Vec<usize>)) {
        let end = self.hir.expr_count();
        let mut inside = HashSet::new();
        for i in pl.first_expr..end {
            match self.hir.kind(ExprId(i as u32)) {
                hir::ExprKind::Field { base, .. } | hir::ExprKind::Len(base) => {
                    inside.insert(*base);
                }
                _ => {}
            }
        }
        for i in pl.first_expr..end {
            let e = ExprId(i as u32);
            if inside.contains(&e)
                || !matches!(
                    self.hir.kind(e),
                    hir::ExprKind::Local(_) | hir::ExprKind::Field { .. }
                )
            {
                continue;
            }
            let Some((root, path)) = self.field_path(e) else { continue };
            let overlaps =
                root == container.0 && path.iter().zip(&container.1).all(|(a, b)| a == b);
            if root.index() >= pl.first_local || !overlaps {
                continue;
            }
            let name = self.hir.local_name(root).to_owned();
            let span = self.hir.span(e);
            self.cx.error_help(
                codes::PARALLEL_LOOP_READS_WRITTEN,
                span,
                format!(
                    "a pass of `parallel for` reaches `{name}`, whose elements the passes write"
                ),
                "another pass may be writing what this reads",
                "each pass reaches the container it loops over only through the element it is \
                 given; read what the loop needs from it before the loop (spec §22.3)",
            );
        }
    }

    /// A place as a local and the fields under it, `w.positions` as `w` and the field's index;
    /// an element or a slice stands for the whole it is in.
    pub(super) fn field_path(&self, e: ExprId) -> Option<(LocalId, Vec<usize>)> {
        match self.hir.kind(e) {
            hir::ExprKind::Local(l) => Some((*l, Vec::new())),
            hir::ExprKind::Field { base, index } => {
                let index = *index;
                let (root, mut path) = self.field_path(*base)?;
                path.push(index);
                Some((root, path))
            }
            hir::ExprKind::Ref { place: base, .. }
            | hir::ExprKind::Deref(base)
            | hir::ExprKind::Index { base, .. }
            | hir::ExprKind::SliceRange { base, .. } => self.field_path(*base),
            _ => None,
        }
    }

    /// The local a node is a place in, through fields, indexing and dereferences.
    fn hir_root(&self, e: ExprId) -> Option<LocalId> {
        match self.hir.kind(e) {
            hir::ExprKind::Local(l) => Some(*l),
            hir::ExprKind::Field { base, .. }
            | hir::ExprKind::TupleIndex { base, .. }
            | hir::ExprKind::Index { base, .. }
            | hir::ExprKind::SliceRange { base, .. }
            | hir::ExprKind::Deref(base) => self.hir_root(*base),
            _ => None,
        }
    }

    /// Report `ty`, which every pass reads (`Share`) or each is given (`Send`), unless it may
    /// cross so. `what` names the local read, if it is one.
    fn pass_crossing(&mut self, span: Span, ty: &Ty, which: Crossing, how: &str, what: &str) {
        if ty.is_wildcard() {
            return;
        }
        let Some(blocker) = self.cx.table.crossing_blocker(ty, which) else { return };
        let name = self.cx.name_of(ty);
        let trait_name = match which {
            Crossing::Share => "Share",
            Crossing::Send => "Send",
        };
        let held = self.cx.describe_blocker(&blocker);
        let help = if self.cx.holds_rc(ty) {
            "every `Rc` of one value shares a count, which threads do not update together yet, \
             so an `Rc` or a `Weak` does not reach `parallel for`; read what the loop needs out \
             of it first (spec §22.4)"
                .to_owned()
        } else if which == Crossing::Share {
            format!(
                "the passes run at the same time on several threads, and each reads what the \
                 loop uses from outside it, so its type is `Share`; `{name}` is not, since \
                 {held} (spec §22.2, §22.3)"
            )
        } else {
            format!(
                "each pass runs on a thread of its own and is given its element, so the \
                 element's type is `Send`; `{name}` is not, since {held} (spec §22.2, §22.3)"
            )
        };
        let message = if what.is_empty() {
            format!("`{name}` may not cross to the threads of `parallel for`")
        } else {
            format!("`{what}`, which {how}, may not cross to the threads of `parallel for`")
        };
        let d = Diagnostic::error(codes::PARALLEL_NOT_SEND, message)
            .at(self.cx.file, span, format!("`{name}` is not `{trait_name}`"))
            .with_help(help);
        self.cx.diags.push(d);
    }

    /// A `return`, or a `break` or `continue` that leaves the pass it is in: no pass can stop
    /// the others (spec §22.3). `target` is the index of the loop a jump reaches, `None` for a
    /// `return`.
    pub(super) fn leaves_pass(&mut self, span: Span, word: &str, target: Option<usize>) -> bool {
        let first_left = match target {
            None => 0,
            Some(d) if word == "continue" => d + 1,
            Some(d) => d,
        };
        if !self.loops.iter().skip(first_left).any(|f| f.parallel) {
            return false;
        }
        self.cx.error_help(
            codes::PARALLEL_LOOP_LEAVES,
            span,
            format!("`{word}` would leave a pass of `parallel for`"),
            "the other passes are running at the same time",
            "no pass can stop the others, so a pass ends only by reaching the end of the body \
             or by `continue`; work out what should stop the loop before it, or loop with `for` \
             (spec §22.3)",
        );
        true
    }
}
