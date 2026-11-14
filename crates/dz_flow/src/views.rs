//! Which loops may read a `List` through one view taken before them (T-0215).
//!
//! `xs[i]` on a `List` is `Index::index`, which checks `i` against the list's own length
//! each time. In a loop that writes elements, C++ cannot tell that a store into the array
//! leaves the list's length and data pointer alone, so it loads both again after every
//! store, checks again, and does not vectorize: `dst[i] = src[i] * 3` over two `&mut List`
//! ran five times slower on clang++ than the same loop over two slices.
//!
//! The tie analysis can tell (spec §6.7). A view of `xs@items` stays good for as long as
//! nothing kills `xs@items`, and every call says what it kills, so when nothing in a loop
//! may kill the list, the slice `xs.as_mut_slice()` would be, taken once before the loop.
//! Codegen then indexes that slice instead. Every index is still checked, against the same
//! length; the check only becomes one the optimiser can see the loop's bound in.
//!
//! The answer is a pure function of one body, as every phase here is (invariant 3), and
//! codegen asks it of each body it emits.

use std::collections::HashMap;

use dz_check::TypeTable;
use dz_hir::ty::Ty;
use dz_hir::{self as hir, Body, Callee, ExprId, LocalId, LocalKind, StmtId};
use dz_resolve::Resolution;
use dz_span::Span;

use crate::ties::analyse_body;

/// For each loop, the `List`s it may index through a view taken before it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LoopViews {
    hoisted: HashMap<StmtId, Vec<Hoisted>>,
}

/// One list a loop indexes through a view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hoisted {
    pub local: LocalId,
    /// Whether the loop writes elements through it (`index_mut`), so the view must be a
    /// writable slice.
    pub mutable: bool,
}

impl LoopViews {
    /// The lists `stmt`, a `while` or a `for`, takes a view of before it starts.
    pub fn at(&self, stmt: StmtId) -> &[Hoisted] {
        self.hoisted.get(&stmt).map_or(&[], Vec::as_slice)
    }

    pub fn is_empty(&self) -> bool {
        self.hoisted.is_empty()
    }
}

/// What a call on a hoisted list becomes: an index into the view, or its length.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewCall {
    Index,
    Len,
}

/// Whether a call to `callee` is one codegen may read through a view of the list `local`:
/// `xs[i]` (`Index::index` or `IndexMut::index_mut`) or `xs.len()`, on the list itself.
pub fn view_call(
    body: &Body,
    table: &TypeTable,
    res: &Resolution,
    callee: &Callee,
) -> Option<(LocalId, ViewCall)> {
    let Callee::Method { recv, owner, name, .. } = callee else { return None };
    let local = list_local(body, res, *recv)?;
    let index = (name == "index" && table.prelude_trait_named("Index") == Some(*owner))
        || (name == "index_mut" && table.prelude_trait_named("IndexMut") == Some(*owner));
    if index {
        return Some((local, ViewCall::Index));
    }
    let list = res.lookup(res.prelude, "List");
    (name == "len" && Some(*owner) == list).then_some((local, ViewCall::Len))
}

/// The local a receiver names, when it is a `List` or a reference to one: `xs`, `*xs`.
fn list_local(body: &Body, res: &Resolution, recv: ExprId) -> Option<LocalId> {
    let local = match body.expr(recv).kind {
        hir::ExprKind::Local(l) => l,
        hir::ExprKind::Deref(inner) => match body.expr(inner).kind {
            hir::ExprKind::Local(l) => l,
            _ => return None,
        },
        _ => return None,
    };
    is_list(res, &body.local(local).ty).then_some(local)
}

fn is_list(res: &Resolution, ty: &Ty) -> bool {
    let list = res.lookup(res.prelude, "List");
    matches!(ty.peeled_refs(), Ty::Named { sym, .. } if Some(*sym) == list)
}

/// The loops of `body` that may index a `List` through a view taken before them.
///
/// A list qualifies for a loop when:
///
/// - it is a local holding the `List` itself, or a parameter holding it or a reference to
///   it. A reference bound inside the body is left out: a kill through what it refers to is
///   recorded against that, not against the reference;
/// - it is declared outside the loop, so it exists before the loop starts;
/// - the loop indexes it, and nothing in the loop, its condition or subject included, kills
///   any part of it or reaches memory the analysis cannot follow (`incomplete`).
///
/// `List`'s own bodies are left out whole: they write `m_len` and `m_data` themselves, which
/// changes the length without killing anything. No other code can, since the fields are
/// private (spec §4). The outermost loop that qualifies takes the view; a loop inside it
/// uses that one.
pub fn loop_views(body: &Body, table: &TypeTable, res: &Resolution) -> LoopViews {
    let mut views = LoopViews::default();
    if body.params.iter().any(|p| {
        let l = body.local(*p);
        l.name == "self" && is_list(res, l.ty.peeled())
    }) {
        return views;
    }
    // Every `xs[i]` on a list that could be viewed, and whether it writes.
    let mut indexed: Vec<(ExprId, LocalId, bool)> = Vec::new();
    for i in 0..body.exprs.len() as u32 {
        let e = ExprId(i);
        let hir::ExprKind::Call { callee, .. } = &body.expr(e).kind else { continue };
        let Some((local, ViewCall::Index)) = view_call(body, table, res, callee) else {
            continue;
        };
        let l = body.local(local);
        let viewable = match l.kind {
            LocalKind::Param => true,
            LocalKind::Let => !matches!(l.ty, Ty::Ref { .. }),
            LocalKind::Binding => false,
        };
        if viewable {
            let writes = matches!(callee, Callee::Method { name, .. } if name == "index_mut");
            indexed.push((e, local, writes));
        }
    }
    let inside = |outer: Span, inner: Span| outer.start <= inner.start && inner.end <= outer.end;
    let loops = loops_outermost_first(body);
    // The tie analysis is the expensive part, so it runs only for a body with a list
    // indexed inside a loop.
    let in_a_loop = indexed
        .iter()
        .any(|(e, _, _)| loops.iter().any(|s| inside(body.stmt(*s).span, body.expr(*e).span)));
    if !in_a_loop {
        return views;
    }
    let analysis = analyse_body(body, table, res);
    let mut taken: Vec<(LocalId, Span)> = Vec::new();
    for s in loops {
        let span = body.stmt(s).span;
        let mut here: Vec<Hoisted> = Vec::new();
        for &(e, local, writes) in &indexed {
            if !inside(span, body.expr(e).span) {
                continue;
            }
            if let Some(h) = here.iter_mut().find(|h| h.local == local) {
                h.mutable |= writes;
                continue;
            }
            let declared_inside =
                body.local(local).kind != LocalKind::Param && inside(span, body.local(local).span);
            let under_view = taken.iter().any(|(l, at)| *l == local && inside(*at, span));
            let killed =
                analysis.kills.iter().any(|k| k.place.local == local && inside(span, k.span));
            let unknown = analysis.incomplete.iter().any(|i| inside(span, i.span));
            if !declared_inside && !under_view && !killed && !unknown {
                here.push(Hoisted { local, mutable: writes });
            }
        }
        for h in &here {
            taken.push((h.local, span));
        }
        if !here.is_empty() {
            views.hoisted.insert(s, here);
        }
    }
    views
}

/// Every `while` and `for` of `body`, each before the loops inside it.
fn loops_outermost_first(body: &Body) -> Vec<StmtId> {
    let mut loops: Vec<StmtId> = (0..body.stmts.len() as u32)
        .map(StmtId)
        .filter(|s| {
            matches!(body.stmt(*s).kind, hir::StmtKind::While { .. } | hir::StmtKind::For { .. })
        })
        .collect();
    // A loop's span holds the spans of the loops inside it, so the widest come first.
    loops.sort_by_key(|s| {
        let span = body.stmt(*s).span;
        (span.start, std::cmp::Reverse(span.end))
    });
    loops
}
