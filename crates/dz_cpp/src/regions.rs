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
//!
//! **What is stored follows its container** (T-0105). A value passed to a `std` method that
//! stores it — `push`, `insert`, `get_or_insert` ([`STORES`]) — is built, or for a `String`
//! copied, with wherever that container keeps its contents. A container says so at run
//! time, through `contents_allocator`, so a function filling a list it was lent asks the
//! list rather than taking a parameter. The answer is the region only for a container the
//! compiler built as one that **keeps** what it is given until it is dropped: placed in a
//! region, and nothing here or in a function it is lent takes an element out, drops one
//! early, or replaces it ([`TAKES`]). Such a container gets a `ScopeShared` view of the
//! region instead of the arena itself; every other container answers with the value's own
//! default, so nothing changes for one given an allocator by hand. Only one level deep: what
//! an element stores goes back to its own default, since an element can give it back.

use std::collections::{HashMap, HashSet};

use dz_hir::ty::Ty;
use dz_hir::{self as hir, BlockId, BodyOwner, ExprId, LocalId};

use crate::Emitter;

/// The name of a function's result allocator in C++.
pub(crate) const RESULT: &str = "_dz_result";

/// The `std` methods that store an argument inside the container they are called on: the
/// container's type, the method, and which arguments, not counting `self`. Written by hand
/// for now, to learn whether inferring them from the bodies is worth it (T-0105).
const STORES: &[(&str, &str, &[usize])] = &[
    ("List", "push", &[0]),
    ("List", "insert", &[1]),
    ("HashMap", "insert", &[0, 1]),
    ("HashMap", "get_or_insert", &[0, 1]),
    ("HashSet", "insert", &[0]),
];

/// The `std` container methods that take an element out or drop one before the container
/// is dropped. Any other method that returns something needing a drop counts as taking one
/// out too, so a new one that hands elements back is caught without being listed.
const TAKES: &[&str] = &[
    "pop",
    "remove",
    "swap_remove",
    "replace",
    "clear",
    "truncate",
    "remove_range",
    "retain",
    "resize",
    "remove_str",
];

/// The containers whose methods [`STORES`] and [`TAKES`] describe.
const CONTAINERS: &[&str] = &["List", "HashMap", "HashSet"];

/// Where one call's allocator comes from, when it is not the heap.
#[derive(Clone, Copy)]
pub(crate) enum Source {
    /// The region of this block.
    Region(BlockId),
    /// The region of this block, lent as a `ScopeShared`: the container built here keeps
    /// what it is given, so that may come from the region too.
    Shared(BlockId),
    /// The enclosing function's result allocator.
    Result,
    /// Wherever the container this storing call is made on keeps its contents.
    Into(ExprId),
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
    /// The blocks that also lend their region as a `ScopeShared`, and the locals holding
    /// the containers built with it.
    pub shares: HashSet<BlockId>,
    pub shared: HashMap<LocalId, BlockId>,
    /// Each `String` copied into a container, and the storing call that copies it.
    pub copies: HashMap<ExprId, ExprId>,
}

/// What surrounds a node.
#[derive(Clone, Copy)]
enum Parent {
    /// Indexed, sliced or iterated: the value stays where it is.
    Kept,
    /// Borrowed by this `&` or `&mut`.
    RefOf { by: ExprId, mutable: bool },
    /// Assigned to: the value stays where it is, and what it held is dropped.
    Assigned,
    /// An argument that this call to a `std` container method stores in the container.
    Stored(ExprId),
    /// This argument of this call to a non-generic free function.
    Arg(ExprId, usize),
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
        let parents = self.surroundings();
        let explain = explain && std::env::var_os("DZ_REGIONS_EXPLAIN").is_some();
        let mut placement = Placement::default();

        // A `String` copied into a container is copied into wherever the container keeps its
        // contents (T-0105).
        if self.regions {
            let mut copies: Vec<(ExprId, ExprId)> = parents
                .iter()
                .filter_map(|(&arg, p)| match p {
                    Parent::Stored(store) if self.is_string_place(arg) => Some((arg, *store)),
                    _ => None,
                })
                .filter(|&(_, store)| self.store_reaches(store))
                .collect();
            copies.sort_by_key(|&(arg, _)| arg.0);
            for (arg, store) in copies {
                if explain {
                    self.explain(arg, Some(Source::Into(store)), None);
                }
                placement.copies.insert(arg, store);
            }
        }
        if sites.is_empty() {
            return (placement, false);
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
                        if self.builds_container(site) && self.keeps(&parents, *local) {
                            placement.shared.insert(*local, *block);
                            (Some(Source::Shared(*block)), None)
                        } else {
                            (Some(Source::Region(*block)), None)
                        }
                    }
                    Fate::Stays => (None, None),
                    Fate::Returned => {
                        returns = true;
                        (own.then_some(Source::Result), None)
                    }
                    Fate::Escapes(at) => (None, Some(at)),
                },
                Some(Parent::Stored(store)) if self.regions && self.store_reaches(*store) => {
                    (Some(Source::Into(*store)), None)
                }
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
                match source {
                    Source::Region(b) => {
                        placement.blocks.insert(b);
                    }
                    Source::Shared(b) => {
                        placement.blocks.insert(b);
                        placement.shares.insert(b);
                    }
                    Source::Result | Source::Into(_) => {}
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
            Some(Parent::Kept | Parent::RefOf { .. } | Parent::Assigned) => None,
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

    /// What surrounds each node of the current body, the calls' arguments included.
    fn surroundings(&self) -> HashMap<ExprId, Parent> {
        let body = self.cur();
        let mut out = parents(body);
        for (i, e) in body.exprs.iter().enumerate() {
            let call = ExprId(i as u32);
            let hir::ExprKind::Call { callee, args } = &e.kind else { continue };
            let stored = self.stored_args(call);
            if !stored.is_empty() {
                for &k in stored {
                    if let Some(&a) = args.get(k) {
                        out.insert(a, Parent::Stored(call));
                    }
                }
            } else if self.result_callee(callee).is_some() {
                for (k, &a) in args.iter().enumerate() {
                    out.insert(a, Parent::Arg(call, k));
                }
            }
        }
        out
    }

    /// Which arguments of `call` it stores in the container it is called on, when it is a
    /// `std` container's storing method.
    fn stored_args(&self, call: ExprId) -> &'static [usize] {
        let body = self.cur();
        let hir::ExprKind::Call { callee: hir::Callee::Method { owner, name, .. }, .. } =
            &body.expr(call).kind
        else {
            return &[];
        };
        let sym = self.res.symbol(*owner);
        if sym.module != self.res.prelude {
            return &[];
        }
        STORES.iter().find(|(o, n, _)| *o == sym.name && n == name).map_or(&[], |(_, _, ks)| ks)
    }

    /// Whether the storing call `store` is made on a place that can be named again to ask
    /// where it keeps its contents.
    fn store_reaches(&self, store: ExprId) -> bool {
        match &self.cur().expr(store).kind {
            hir::ExprKind::Call { callee: hir::Callee::Method { recv, .. }, .. } => {
                self.fixed_place(*recv)
            }
            _ => false,
        }
    }

    /// Whether `e` reads a `String` from a place, so passing it by value copies it.
    fn is_string_place(&self, e: ExprId) -> bool {
        let body = self.cur();
        let place =
            matches!(body.expr(e).kind, hir::ExprKind::Local(_) | hir::ExprKind::Field { .. });
        place
            && self.fixed_place(e)
            && matches!(body.ty(e).peeled(), Ty::Named { sym, .. } if self.is_std(*sym, "String"))
    }

    fn is_std(&self, sym: dz_resolve::SymbolId, name: &str) -> bool {
        let s = self.res.symbol(sym);
        s.module == self.res.prelude && s.name == name
    }

    /// Whether `site` builds one of the containers [`STORES`] describes, of something that
    /// owns memory of its own: a `List<i32>` has no contents to place.
    fn builds_container(&self, site: ExprId) -> bool {
        let body = self.cur();
        let ctor = matches!(
            &body.expr(site).kind,
            hir::ExprKind::Call { callee: hir::Callee::Ctor { owner, .. }, .. }
                if CONTAINERS.iter().any(|c| self.is_std(*owner, c))
        );
        ctor && matches!(body.ty(site), Ty::Named { args, .. } if args.iter().any(|a| self.table.needs_drop(a)))
    }

    /// Whether the container in `local` keeps everything stored in it until it is itself
    /// dropped: nothing here takes an element out, drops one early or replaces the
    /// container, and every function it is lent to does the same.
    fn keeps(&mut self, parents: &HashMap<ExprId, Parent>, local: LocalId) -> bool {
        let body = self.cur();
        let uses: Vec<ExprId> = (0..body.exprs.len() as u32)
            .map(ExprId)
            .filter(|&e| matches!(body.expr(e).kind, hir::ExprKind::Local(l) if l == local))
            .collect();
        uses.into_iter().all(|id| match parents.get(&id) {
            Some(Parent::Kept | Parent::RefOf { mutable: false, .. }) => true,
            Some(Parent::Receiver(call)) => self.keeping_method(*call),
            Some(Parent::RefOf { by, mutable: true }) => match parents.get(by) {
                Some(Parent::Arg(call, k)) => self.keeps_param(*call, *k),
                _ => false,
            },
            // A reference parameter passed on.
            Some(Parent::Arg(call, k)) => self.keeps_param(*call, *k),
            _ => false,
        })
    }

    /// Whether the method `call` calls leaves every element where it is: a `std` method
    /// that neither takes one out nor drops one.
    fn keeping_method(&self, call: ExprId) -> bool {
        let body = self.cur();
        let hir::ExprKind::Call { callee: hir::Callee::Method { owner, name, .. }, .. } =
            &body.expr(call).kind
        else {
            return false;
        };
        self.res.symbol(*owner).module == self.res.prelude
            && !TAKES.contains(&name.as_str())
            && !self.table.needs_drop(body.ty(call))
    }

    /// Whether the function `call` calls keeps everything stored in the container its
    /// parameter `k` is lent, by the same rules as [`Emitter::keeps`] applied to its body:
    /// the per-parameter summary, inferred, and decided once for the whole program. A
    /// function asked about while it is still being decided — recursion — does not.
    fn keeps_param(&mut self, call: ExprId, k: usize) -> bool {
        let hir::ExprKind::Call { callee, .. } = &self.cur().expr(call).kind else { return false };
        let Some(callee_body) = self.result_callee(callee) else { return false };
        let Some(&param) = callee_body.params.get(k) else { return false };
        if !matches!(callee_body.local(param).ty, Ty::Ref { .. }) {
            return false;
        }
        let key = (callee_body as *const hir::Body as usize, k);
        if let Some(known) = self.keeps.borrow().get(&key) {
            return *known;
        }
        self.keeps.borrow_mut().insert(key, false);
        let saved = self.body.replace(callee_body);
        let parents = self.surroundings();
        let kept = self.keeps(&parents, param);
        self.body = saved;
        self.keeps.borrow_mut().insert(key, kept);
        kept
    }

    /// The allocator for a value the storing call `store` keeps: the region itself when
    /// the container is one this body built to share it, else what the container says at
    /// run time, `fallback` when it has no opinion.
    pub(crate) fn stored_allocator(&mut self, store: ExprId, fallback: &str) -> String {
        let body = self.cur();
        let hir::ExprKind::Call { callee: hir::Callee::Method { recv, .. }, .. } =
            &body.expr(store).kind
        else {
            return fallback.to_owned();
        };
        if let hir::ExprKind::Local(l) = body.expr(*recv).kind {
            if let Some(b) = self.placement.shared.get(&l) {
                return format!("_dz_region{}", b.index());
            }
        }
        let r = self.expr(*recv);
        format!("::dz::obj({r}).contents_allocator({fallback})")
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
            _ => "a copy".to_owned(),
        };
        match (source, why) {
            (Some(Source::Region(_)), _) => eprintln!("region  {place}  {what}"),
            (Some(Source::Shared(_)), _) => {
                eprintln!("region  {place}  {what}: and what it stores, which it keeps")
            }
            (Some(Source::Result), _) => eprintln!("result  {place}  {what}"),
            (Some(Source::Into(store)), _) => {
                let into = match &body.expr(store).kind {
                    hir::ExprKind::Call { callee: hir::Callee::Method { name, .. }, .. } => {
                        name.as_str()
                    }
                    _ => "",
                };
                eprintln!("into    {place}  {what}: wherever .{into} keeps it")
            }
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
            hir::ExprKind::Ref { place, mutable, .. } => {
                parents.insert(*place, Parent::RefOf { by: id, mutable: *mutable });
            }
            hir::ExprKind::TupleIndex { base, .. }
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
                parents.insert(*target, Parent::Assigned);
            }
            hir::StmtKind::For { subject, .. } => {
                parents.insert(*subject, Parent::Kept);
            }
            _ => {}
        }
    }
    parents
}
