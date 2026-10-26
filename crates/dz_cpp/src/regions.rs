//! Regions, a prototype (T-0104): where the memory of what a call returns comes from.
//!
//! Two kinds of call leave that to the compiler:
//!
//! * a call to something with a parameter whose default is `#allocator` —
//!   `allocator: ^mut Allocator = #allocator` — that leaves it out. The parameter is an
//!   ordinary one: the callee passes it on explicitly, to what it builds;
//! * a call to a function that builds what it returns without being told where: a
//!   `List<T>()` it returns, or the result of another such call it returns. The function
//!   gets a **result allocator**, a hidden last parameter its signature does not show.
//!
//! Each such call is given the allocator for what it returns:
//!
//! * **the scope's region**, when the result is bound by `let x = …` (or `let x = … or …`)
//!   and `x` is only ever borrowed afterwards: `&x`, a method that takes `self` by
//!   reference, `x[i]`, a slice, a `for`, an assignment to it, or a field read that costs
//!   nothing to copy. The region is declared first in the block holding the `let`, so it is
//!   released after everything in it has been dropped, never before. Only with `DZ_REGIONS`
//!   set, read by `dz`;
//! * **the enclosing function's result allocator**, when the result is what that function
//!   returns, so the choice made where *it* was called flows through;
//! * **the heap** otherwise: `Allocators::Global`.
//!
//! Prototype limits: only a non-generic free function gets a result allocator, and not one
//! also used as a function value, whose C++ type must stay as written. `DZ_REGIONS_EXPLAIN`
//! prints every decision.
//!
//! A copy made through a borrow is the library's to handle: a container copied out of a
//! region goes to the heap (`Allocator::scoped`), since nothing says the copy dies with the
//! scope.

use std::collections::{HashMap, HashSet};

use dz_hir::ty::Ty;
use dz_hir::{self as hir, BlockId, BodyOwner, ExprId, LocalId};

use crate::Emitter;

/// The name of a function's result allocator in C++.
pub(crate) const RESULT: &str = "_dz_result";

/// Where one call's allocator comes from, when it is not the heap.
#[derive(Clone, Copy)]
pub(crate) enum Source {
    /// The region of this block.
    Region(BlockId),
    /// The enclosing function's result allocator.
    Result,
}

/// Which argument a call's allocator is.
#[derive(Clone, Copy)]
pub(crate) enum Slot {
    /// The callee's `#allocator` parameter, by index.
    Param(usize),
    /// The callee's result allocator, after every other argument.
    Hidden,
}

/// What the analysis decided for one body.
#[derive(Default)]
pub(crate) struct Placement {
    /// Each call whose allocator is decided here, and where it comes from.
    pub sites: HashMap<ExprId, (Slot, Source)>,
    /// The blocks that declare a region.
    pub blocks: HashSet<BlockId>,
}

/// What surrounds a node.
#[derive(Clone, Copy)]
enum Parent {
    /// Borrowed, indexed, sliced, iterated or assigned to: the value stays where it is.
    Kept,
    /// The receiver of this method call.
    Receiver(ExprId),
    /// The base of this field access.
    FieldOf(ExprId),
    /// This `move(…)`.
    Move(ExprId),
    /// `return`'s value.
    Return,
    /// The initialiser of a `let` binding this local, in this block.
    LetInit(LocalId, BlockId),
    /// The operand of this `or`, whose success value is the operand's own.
    OrOperand(ExprId),
}

/// What becomes of a value bound by `let`.
enum Fate {
    /// Only borrowed: it can live in the scope's region.
    Stays,
    /// Returned, and otherwise only borrowed.
    Returned,
    /// Used as a value here: passed, moved, copied or bound elsewhere.
    Escapes(ExprId),
}

impl<'a> Emitter<'a> {
    /// Decide where the current body's calls get their allocators.
    pub(crate) fn place_regions(&mut self) -> Placement {
        let body = self.cur();
        let own = self.places_result(body);
        self.decide(own, true).0
    }

    /// Whether `body` has a result allocator: it is a function that may have one, and it
    /// returns something built by a call that leaves the allocator to the compiler.
    /// Decided once per body and shared by every module, so a signature and every call to
    /// it agree. A body asked about while it is still being decided — recursion — counts as
    /// having none, which costs only a missed placement.
    pub(crate) fn places_result(&mut self, body: &'a hir::Body) -> bool {
        let BodyOwner::Fn(sym) = body.owner else { return false };
        let generic = match self.table.get(sym) {
            Some(dz_check::ItemSig::Fn(sig)) => !sig.generics.is_empty(),
            _ => true,
        };
        if generic
            || self.fn_values.contains(&sym)
            || self.is_foreign(sym)
            || !self.table.needs_drop(&body.ret)
        {
            return false;
        }
        let key = body as *const hir::Body as usize;
        if let Some(known) = self.results.borrow().get(&key) {
            return *known;
        }
        self.results.borrow_mut().insert(key, false);
        let saved = self.body.replace(body);
        let returns = self.decide(true, false).1;
        self.body = saved;
        self.results.borrow_mut().insert(key, returns);
        returns
    }

    /// The current body's decisions, and whether any call's result is what it returns.
    /// `own` says whether the body has a result allocator to hand on; `explain` whether to
    /// print the decisions.
    fn decide(&mut self, own: bool, explain: bool) -> (Placement, bool) {
        let body = self.cur();
        let mut sites = Vec::new();
        for i in 0..body.exprs.len() {
            let e = ExprId(i as u32);
            if let Some(slot) = self.allocator_slot(e) {
                sites.push((e, slot));
            }
        }
        if sites.is_empty() {
            return (Placement::default(), false);
        }

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
        let parents = parents(body);
        let explain = explain && std::env::var_os("DZ_REGIONS_EXPLAIN").is_some();

        let mut placement = Placement::default();
        let mut returns = false;
        for (site, slot) in sites {
            // `let x = make() or …` binds what `make` returned, when it succeeded.
            let mut around = parents.get(&site);
            while let Some(Parent::OrOperand(or)) = around {
                around = parents.get(or);
            }
            let returned = match around {
                Some(Parent::Return) => true,
                Some(Parent::Move(m)) => matches!(parents.get(m), Some(Parent::Return)),
                _ => false,
            };
            let (source, why) = match around {
                Some(Parent::LetInit(local, block)) => match self.fate(&parents, *local) {
                    Fate::Stays if self.regions && !lambda_bodies.contains(block) => {
                        (Some(Source::Region(*block)), None)
                    }
                    Fate::Stays => (None, None),
                    Fate::Returned => {
                        returns = true;
                        (own.then_some(Source::Result), None)
                    }
                    Fate::Escapes(at) => (None, Some(at)),
                },
                _ if returned => {
                    returns = true;
                    (own.then_some(Source::Result), None)
                }
                _ => (None, Some(site)),
            };
            if explain {
                self.explain(site, source, why);
            }
            if let Some(source) = source {
                if let Source::Region(b) = source {
                    placement.blocks.insert(b);
                }
                placement.sites.insert(site, (slot, source));
            }
        }
        (placement, returns)
    }

    /// Which argument of `e` is the allocator for its result, when `e` is a call that leaves
    /// that to the compiler.
    fn allocator_slot(&mut self, e: ExprId) -> Option<Slot> {
        let body = self.cur();
        let node = body.expr(e);
        let hir::ExprKind::Call { callee, args } = &node.kind else { return None };
        if let Some(callee_body) = self.result_callee(callee) {
            if self.places_result(callee_body) {
                return Some(Slot::Hidden);
            }
        }
        let defaults = self.callee_defaults(callee, args, &node.ty);
        let index = defaults.iter().position(|d| {
            d.is_some_and(|(callee_body, default)| hir::is_allocator(callee_body, default))
        })?;
        let left_out = match args.get(index) {
            None => true,
            Some(a) => {
                matches!(body.expr(*a).kind, hir::ExprKind::DefaultArg(_))
                    || hir::is_allocator(body, *a)
            }
        };
        left_out.then_some(Slot::Param(index))
    }

    /// The body of the function `callee` calls, when it is one that could have a result
    /// allocator: a non-generic free function.
    pub(crate) fn result_callee(&self, callee: &hir::Callee) -> Option<&'a hir::Body> {
        match callee {
            hir::Callee::Fn { sym, type_args } if type_args.is_empty() => {
                self.hir.find(&BodyOwner::Fn(*sym))
            }
            _ => None,
        }
    }

    /// What becomes of `local`'s value: the worst of what each of its uses does with it.
    fn fate(&self, parents: &HashMap<ExprId, Parent>, local: LocalId) -> Fate {
        let body = self.cur();
        let mut returned = false;
        for (i, e) in body.exprs.iter().enumerate() {
            if !matches!(e.kind, hir::ExprKind::Local(l) if l == local) {
                continue;
            }
            let id = ExprId(i as u32);
            match parents.get(&id) {
                Some(Parent::Return) => returned = true,
                Some(Parent::Move(m)) if matches!(parents.get(m), Some(Parent::Return)) => {
                    returned = true
                }
                _ => {
                    if let Some(at) = self.used_as_value(parents, id) {
                        return Fate::Escapes(at);
                    }
                }
            }
        }
        if returned {
            Fate::Returned
        } else {
            Fate::Stays
        }
    }

    /// Where the place `id` — a local, or a field path from one — is used as a value, if it
    /// is: `None` when every use leaves it where it is.
    fn used_as_value(&self, parents: &HashMap<ExprId, Parent>, id: ExprId) -> Option<ExprId> {
        match parents.get(&id) {
            Some(Parent::Kept) => None,
            Some(Parent::Receiver(call)) if self.self_by_reference(*call) => None,
            // A field read is a copy of the field: free unless the field owns something, and
            // then it is the field that must stay put.
            Some(Parent::FieldOf(field)) => {
                if self.copy_is_free(self.cur().ty(*field)) {
                    None
                } else {
                    self.used_as_value(parents, *field)
                }
            }
            _ => Some(id),
        }
    }

    /// Whether the method `call` calls takes `self` by reference, so calling it leaves the
    /// receiver where it is. Every method `std` declares does.
    fn self_by_reference(&self, call: ExprId) -> bool {
        let body = self.cur();
        let hir::ExprKind::Call {
            callee: hir::Callee::Method { recv, owner, name, method_args },
            ..
        } = &body.expr(call).kind
        else {
            return false;
        };
        if self.res.symbol(*owner).module == self.res.prelude {
            return true;
        }
        let callee = if self.res.symbol(*owner).kind == dz_resolve::SymbolKind::Trait {
            self.hir
                .find(&BodyOwner::TraitDefaults { owner: *owner, name: name.clone() })
                .or_else(|| self.hir.find(&BodyOwner::Method { owner: *owner, name: name.clone() }))
        } else {
            let owner_args = match body.ty(*recv).peeled() {
                Ty::Named { sym, args } if sym == owner => args.clone(),
                _ => Vec::new(),
            };
            self.method_body(*owner, &owner_args, name, method_args)
        };
        callee.is_some_and(|b| {
            b.params.iter().any(|p| {
                let l = b.local(*p);
                l.name == "self" && matches!(l.ty, Ty::Ref { .. })
            })
        })
    }

    fn explain(&self, site: ExprId, source: Option<Source>, why: Option<ExprId>) {
        let body = self.cur();
        let file = self.map.get(body.file);
        let at = file.location(body.expr(site).span.start as usize);
        let place = format!("{}:{}:{}", file.name(), at.line, at.column);
        let what = match &body.expr(site).kind {
            hir::ExprKind::Call { callee, .. } => match callee {
                hir::Callee::Fn { sym, .. } => self.res.symbol(*sym).name.clone(),
                hir::Callee::Ctor { owner, .. } => self.res.symbol(*owner).name.clone(),
                hir::Callee::Assoc { owner, name, .. } => {
                    format!("{}::{name}", self.res.symbol(*owner).name)
                }
                hir::Callee::Method { name, .. } | hir::Callee::BaseMethod { name, .. } => {
                    format!(".{name}")
                }
                hir::Callee::Value(_) => "a function value".to_owned(),
            },
            _ => String::new(),
        };
        match (source, why) {
            (Some(Source::Region(_)), _) => eprintln!("region  {place}  {what}"),
            (Some(Source::Result), _) => eprintln!("result  {place}  {what}"),
            (None, Some(u)) => {
                let w = file.location(body.expr(u).span.start as usize);
                eprintln!("heap    {place}  {what}: used as a value at {}:{}", w.line, w.column)
            }
            (None, None) => eprintln!("heap    {place}  {what}"),
        }
    }
}

/// Every function used as a value anywhere: its C++ type is fixed by the type it is used
/// as, so it cannot take a result allocator.
pub(crate) fn fn_values(hir: &hir::Hir) -> HashSet<dz_resolve::SymbolId> {
    let mut out = HashSet::new();
    for (_, body) in hir.bodies() {
        for e in &body.exprs {
            if let hir::ExprKind::FnItem { sym, .. } = e.kind {
                out.insert(sym);
            }
        }
    }
    out
}

/// What surrounds each node that is used in place, returned, or bound by `let`. A node
/// missing here is used as a value.
fn parents(body: &hir::Body) -> HashMap<ExprId, Parent> {
    let mut parents = HashMap::new();
    for (i, e) in body.exprs.iter().enumerate() {
        let id = ExprId(i as u32);
        match &e.kind {
            hir::ExprKind::Ref { place: base, .. }
            | hir::ExprKind::TupleIndex { base, .. }
            | hir::ExprKind::Index { base, .. }
            | hir::ExprKind::SliceRange { base, .. }
            | hir::ExprKind::Len(base)
            | hir::ExprKind::DataPtr(base) => {
                parents.insert(*base, Parent::Kept);
            }
            hir::ExprKind::Field { base, .. } => {
                parents.insert(*base, Parent::FieldOf(id));
            }
            hir::ExprKind::Call {
                callee: hir::Callee::Method { recv, .. } | hir::Callee::BaseMethod { recv, .. },
                ..
            } => {
                parents.insert(*recv, Parent::Receiver(id));
            }
            hir::ExprKind::Move(inner) => {
                parents.insert(*inner, Parent::Move(id));
            }
            hir::ExprKind::Return(Some(v)) => {
                parents.insert(*v, Parent::Return);
            }
            hir::ExprKind::Or { operand, .. } => {
                parents.insert(*operand, Parent::OrOperand(id));
            }
            _ => {}
        }
    }
    for (i, block) in body.blocks.iter().enumerate() {
        for s in &block.stmts {
            if let hir::StmtKind::Let { local: Some(local), init: Some(init) } = &body.stmt(*s).kind
            {
                parents.insert(*init, Parent::LetInit(*local, BlockId(i as u32)));
            }
        }
    }
    for s in &body.stmts {
        match &s.kind {
            hir::StmtKind::Assign { target, .. } => {
                parents.insert(*target, Parent::Kept);
            }
            hir::StmtKind::For { subject, .. } => {
                parents.insert(*subject, Parent::Kept);
            }
            _ => {}
        }
    }
    parents
}
