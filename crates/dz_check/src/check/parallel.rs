//! `parallel { … }`: calls that run at the same time (spec §22.3, D-0118).
//!
//! The checker holds each statement of the block to one call that returns nothing, keeps a
//! function that needs the main thread out of it, and lets an argument cross only as §22.2
//! says: as `&T` when `T` is `Share`, as `&mut T` or by value when `T` is `Send`. Whether two
//! calls reach the same place is the ties analysis's question, asked in `dz_flow`.

use dz_ast::{Block, ExprKind, StmtKind};
use dz_diag::{codes, Diagnostic};
use dz_hir::{self as hir, ExprId};
use dz_span::Span;

use super::Body;
use crate::threads::Crossing;
use crate::ty::Ty;

impl Body<'_, '_> {
    pub(super) fn parallel_stmt(&mut self, b: &Block) -> hir::StmtKind {
        let mut calls = Vec::new();
        for s in &b.stmts {
            let call = match &s.kind {
                StmtKind::Expr(e) if matches!(e.kind, ExprKind::Call { .. }) => e,
                _ => {
                    self.not_a_call(s.span, "this is not a call");
                    continue;
                }
            };
            let saved = self.parallel_call.replace(call.span);
            self.stmt_position = true;
            let c = self.expr(call, None);
            self.stmt_position = false;
            self.parallel_call = saved;
            for (span, why) in std::mem::take(&mut self.parallel_needs) {
                self.cx.error_help(
                    codes::PARALLEL_MAIN_THREAD,
                    span,
                    "a call in `parallel` runs on another thread, and this one needs the main thread",
                    why,
                    "every call in `parallel` but the first runs on a thread of its own, so none \
                     may touch a plain static or call C that needs the main thread; call it \
                     before or after the block (spec §22.3, §22.5)",
                );
            }
            if !matches!(self.hir.kind(c.id), hir::ExprKind::Call { .. } | hir::ExprKind::Error) {
                self.not_a_call(call.span, "this is not a call of a function");
                continue;
            }
            if !matches!(c.ty, Ty::Void | Ty::Error | Ty::Never) {
                let ty = self.cx.name_of(&c.ty);
                self.cx.error_help(
                    codes::PARALLEL_NOT_A_CALL,
                    call.span,
                    "a call in `parallel` returns nothing",
                    format!("this returns `{ty}`"),
                    "`parallel` does not hand results back yet, so each call returns nothing; \
                     write the result through a `&mut` argument instead (spec §22.3)",
                );
                continue;
            }
            self.parallel_crossings(c.id);
            calls.push(c.id);
        }
        hir::StmtKind::Parallel { calls }
    }

    fn not_a_call(&mut self, span: Span, label: &str) {
        self.cx.error_help(
            codes::PARALLEL_NOT_A_CALL,
            span,
            "a statement in `parallel` is one call",
            label,
            "each line of `parallel { … }` is a call to a function or a method, and the calls \
             run at the same time; work out what they need before the block (spec §22.3)",
        );
    }

    /// Hold each argument of the call `id`, and its receiver, to what may cross to another
    /// thread (spec §22.2, §22.3).
    fn parallel_crossings(&mut self, id: ExprId) {
        let hir::ExprKind::Call { callee, args } = self.hir.kind(id).clone() else { return };
        if let hir::Callee::Method { recv, owner, name, .. }
        | hir::Callee::BaseMethod { recv, owner, name } = &callee
        {
            let ty = self.hir.ty(*recv).peeled_refs().clone();
            let takes_self = self.cx.table.methods_sig(*owner).and_then(|s| {
                let key = s.overloads.get(name).and_then(|keys| keys.first()).unwrap_or(name);
                s.methods.get(key).and_then(|m| m.takes_self)
            });
            let span = self.hir.span(*recv);
            match takes_self {
                Some(true) => self.crossing(span, &ty, Crossing::Send, "`&mut self`"),
                Some(false) => self.crossing(span, &ty, Crossing::Share, "`&self`"),
                None => {
                    self.crossing(span, &ty, Crossing::Send, "`self`");
                    self.crossing(span, &ty, Crossing::Share, "`self`");
                }
            }
        }
        for a in args {
            if matches!(self.hir.kind(a), hir::ExprKind::DefaultArg(_)) {
                continue;
            }
            let span = self.hir.span(a);
            match self.hir.ty(a).clone() {
                Ty::Ref { mutable: false, inner } => {
                    self.crossing(span, &inner, Crossing::Share, "`&`")
                }
                Ty::Slice { mutable: false, elem } | Ty::ConstSlice(elem) => {
                    self.crossing(span, &elem, Crossing::Share, "`&`")
                }
                Ty::Ref { mutable: true, inner } => {
                    self.crossing(span, &inner, Crossing::Send, "`&mut`")
                }
                Ty::Slice { mutable: true, elem } => {
                    self.crossing(span, &elem, Crossing::Send, "`&mut`")
                }
                ty => self.crossing(span, &ty, Crossing::Send, "value"),
            }
        }
    }

    /// Report `ty`, lent to another thread as `how`, unless it crosses as `which`.
    fn crossing(&mut self, span: Span, ty: &Ty, which: Crossing, how: &str) {
        if ty.is_wildcard() {
            return;
        }
        let Some(blocker) = self.cx.table.parallel_crossing_blocker(ty, which) else { return };
        let name = self.cx.name_of(ty);
        let (trait_name, rule) = match which {
            Crossing::Share => (
                "Share",
                "an argument lent as `&` is read by another thread while this one may read it \
                 too, so its type is `Share`",
            ),
            Crossing::Send => (
                "Send",
                "an argument given by value or lent as `&mut` is the other thread's while the \
                 call runs, so its type is `Send`",
            ),
        };
        let held = self.cx.describe_blocker(&blocker);
        let lent =
            if how == "value" { "given by value".to_owned() } else { format!("lent as {how}") };
        let help = format!("{rule}; `{name}` is not, since {held} (spec §22.2, §22.3)");
        let d = Diagnostic::error(
            codes::PARALLEL_NOT_SEND,
            format!("`{name}` may not cross to another thread {lent}"),
        )
        .at(self.cx.file, span, format!("`{name}` is not `{trait_name}`"))
        .with_help(help);
        self.cx.diags.push(d);
    }
}
