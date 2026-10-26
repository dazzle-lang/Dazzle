//! Regions, a prototype (T-0104): which containers a scope builds and never lets out, so
//! their memory can come from an arena released when the scope ends instead of the heap.
//!
//! Nothing here changes what a program means. The owner still drops its value at the end
//! of its scope (spec §6.5); only where the memory comes from changes, and the region is
//! released after every value in it has been dropped, never before. Off unless the build
//! asks for it (`DZ_REGIONS`, read by `dz`), and `DZ_REGIONS_EXPLAIN` prints each decision.
//!
//! Stage 1 of the task only: a value is placed when it is
//!
//! * built by `let x = List<T>(…)` — `String`, `HashMap` and `HashSet` likewise, or
//!   `String::from(…)` — with the allocator left to its default, and
//! * only ever borrowed afterwards: `&x`, a method of `std` called on it, `x[i]`, a slice of
//!   it, a `for` over it, or an assignment to it.
//!
//! Anything else — `x` returned, moved, passed by value, copied — keeps the heap. A copy
//! made through a borrow is the library's to handle: a container copied out of a region goes
//! to the heap (`Allocator::scoped`), since nothing says the copy dies with the scope.

use std::collections::{HashMap, HashSet};

use dz_hir::ty::Ty;
use dz_hir::{self as hir, BlockId, ExprId, LocalId};

use crate::Emitter;

/// The containers whose construction can be placed. Every method `std` gives them takes
/// `self` by reference, which is what lets a method call count as a borrow.
const PLACEABLE: [&str; 4] = ["List", "String", "HashMap", "HashSet"];

/// What the analysis decided for one body.
#[derive(Default)]
pub(crate) struct Placement {
    /// Each placed construction: the block whose region it uses, and the index of the
    /// `allocator` argument the region replaces.
    pub sites: HashMap<ExprId, (BlockId, usize)>,
    /// The blocks that declare a region.
    pub blocks: HashSet<BlockId>,
}

/// How the expression or statement around a node uses its value.
#[derive(Clone, Copy)]
enum Use {
    /// Borrowed, indexed, sliced, iterated or assigned to: the value stays where it is.
    Kept,
    /// The receiver of this method call.
    Receiver(ExprId),
}

/// A `let` that might be placed: the local, the construction, its block and the index of
/// its `allocator` parameter.
struct Candidate {
    local: LocalId,
    site: ExprId,
    block: BlockId,
    allocator: usize,
}

impl Emitter<'_> {
    /// Decide which of the current body's constructions use a region.
    pub(crate) fn place_regions(&self) -> Placement {
        let body = self.cur();
        // A formatting macro's body is emitted as a lambda, not through `block_into`, so
        // nothing declares a region at its top.
        let lambda_bodies: HashSet<BlockId> = body
            .exprs
            .iter()
            .filter_map(|e| match &e.kind {
                hir::ExprKind::Formatted { body, .. } => Some(*body),
                _ => None,
            })
            .collect();

        let mut candidates = Vec::new();
        for (i, block) in body.blocks.iter().enumerate() {
            let b = BlockId(i as u32);
            if lambda_bodies.contains(&b) {
                continue;
            }
            for s in &block.stmts {
                let hir::StmtKind::Let { local: Some(local), init: Some(init) } =
                    &body.stmt(*s).kind
                else {
                    continue;
                };
                if let Some(allocator) = self.placeable(*local, *init) {
                    candidates.push(Candidate { local: *local, site: *init, block: b, allocator });
                }
            }
        }
        if candidates.is_empty() {
            return Placement::default();
        }

        let uses = uses(body);
        let mut placement = Placement::default();
        let explain = std::env::var_os("DZ_REGIONS_EXPLAIN").is_some();
        for c in candidates {
            let escape = body.exprs.iter().enumerate().find_map(|(i, e)| {
                let id = ExprId(i as u32);
                match e.kind {
                    hir::ExprKind::Local(l) if l == c.local => match uses.get(&id) {
                        Some(Use::Kept) => None,
                        Some(Use::Receiver(call)) if self.std_method(*call) => None,
                        _ => Some(id),
                    },
                    _ => None,
                }
            });
            if explain {
                let file = self.map.get(body.file);
                let name = &body.local(c.local).name;
                let at = file.location(body.expr(c.site).span.start as usize);
                let place = format!("{}:{}:{}", file.name(), at.line, at.column);
                match escape {
                    None => eprintln!("region  {place}  {name}"),
                    Some(u) => {
                        let why = file.location(body.expr(u).span.start as usize);
                        eprintln!(
                            "heap    {place}  {name}: used as a value at line {}:{}",
                            why.line, why.column
                        );
                    }
                }
            }
            if escape.is_none() {
                placement.sites.insert(c.site, (c.block, c.allocator));
                placement.blocks.insert(c.block);
            }
        }
        placement
    }

    /// The index of `init`'s `allocator` argument, when `init` builds one of the
    /// [`PLACEABLE`] containers for `local` and leaves that argument to its default.
    fn placeable(&self, local: LocalId, init: ExprId) -> Option<usize> {
        let body = self.cur();
        let node = body.expr(init);
        let hir::ExprKind::Call { callee, args } = &node.kind else { return None };
        let owner = match callee {
            hir::Callee::Ctor { owner, .. } => *owner,
            hir::Callee::Assoc { owner, name, .. } if name == "from" => *owner,
            _ => return None,
        };
        let sym = self.res.symbol(owner);
        if sym.module != self.res.prelude || !PLACEABLE.contains(&sym.name.as_str()) {
            return None;
        }
        // `String::from_utf8` and the like build something else around the container.
        if !matches!(&body.local(local).ty, Ty::Named { sym, .. } if *sym == owner) {
            return None;
        }
        let defaults = self.callee_defaults(callee, args, &node.ty);
        let index = defaults.iter().position(|d| {
            d.is_some_and(|(callee_body, default)| {
                matches!(
                    &callee_body.expr(default).kind,
                    hir::ExprKind::AssocConst { name, .. } if name == "Global"
                )
            })
        })?;
        let left_out = match args.get(index) {
            None => true,
            Some(a) => matches!(body.expr(*a).kind, hir::ExprKind::DefaultArg(_)),
        };
        left_out.then_some(index)
    }

    /// Whether `call` calls a method `std` declares, none of which takes `self` by value.
    fn std_method(&self, call: ExprId) -> bool {
        match &self.cur().expr(call).kind {
            hir::ExprKind::Call { callee: hir::Callee::Method { owner, .. }, .. } => {
                self.res.symbol(*owner).module == self.res.prelude
            }
            _ => false,
        }
    }
}

/// How each node that is used in place is used. A node missing here is used as a value:
/// passed, returned, moved, copied or bound.
fn uses(body: &hir::Body) -> HashMap<ExprId, Use> {
    let mut uses = HashMap::new();
    for (i, e) in body.exprs.iter().enumerate() {
        let id = ExprId(i as u32);
        match &e.kind {
            hir::ExprKind::Ref { place: base, .. }
            | hir::ExprKind::Field { base, .. }
            | hir::ExprKind::TupleIndex { base, .. }
            | hir::ExprKind::Index { base, .. }
            | hir::ExprKind::SliceRange { base, .. }
            | hir::ExprKind::Len(base)
            | hir::ExprKind::DataPtr(base) => {
                uses.insert(*base, Use::Kept);
            }
            hir::ExprKind::Call {
                callee: hir::Callee::Method { recv, .. } | hir::Callee::BaseMethod { recv, .. },
                ..
            } => {
                uses.insert(*recv, Use::Receiver(id));
            }
            _ => {}
        }
    }
    for s in &body.stmts {
        match &s.kind {
            hir::StmtKind::Assign { target, .. } => {
                uses.insert(*target, Use::Kept);
            }
            hir::StmtKind::For { subject, .. } => {
                uses.insert(*subject, Use::Kept);
            }
            _ => {}
        }
    }
    uses
}
