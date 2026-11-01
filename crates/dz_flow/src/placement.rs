//! Where each allocation goes: a scope's region, the caller's result placement, or the heap
//! (spec §14.6, T-0117 stages 1 and 2).
//!
//! A **site** is a call to a `std` function with an allocator parameter left at its default,
//! `Allocators::Global`: `List<T>()`, `String(…)`, `HashMap`, `HashSet`, `SlotMap`, `Path`,
//! `Box`, `Rc`, `String::from`, `String::from_utf8`. A call to the program's own function with
//! such a parameter is a site too, and always heap: what that function does with its
//! allocator is out of sight here.
//!
//! A call to a **result function** is a site as well (stage 2). That is a function whose
//! result owns memory and which takes, out of sight, where to build it: a hidden last
//! parameter, `_dz_result` in the C++ ([`result_functions`] says which take one). Inside
//! one, a site whose value is returned is built there ([`Decision::Result`]); at a call,
//! the caller passes its region, its own result placement, or the heap, decided as for any
//! site.
//!
//! Each site is decided in two steps, per body and from nothing but that body and the
//! signatures it calls, so the editor, `dz placements` and codegen read one answer:
//!
//! 1. **The proposer** (the prototype's stage-1 rule, `dz_cpp/src/regions.rs` at the tag
//!    `regions-prototype`) proposes the region of block `B` for `let x = site` in `B` when
//!    every later use of `x` leaves it where it is: borrowed, indexed, sliced, measured,
//!    walked by a `for`, assigned to, a method's receiver, a field read, or copied.
//! 2. **The region check** walks the body's ties again with the proposals as facts
//!    ([`crate::ties::analyse_regions`]) and sends every placement it refuses to the heap,
//!    its finding as the reason. It shares no rule with the proposer: it is the safety net
//!    the prototype lacked, and a placement both accept is one the tie analysis can follow.
//!
//! Regions are per block and only `let` initialisers are proposed, so a placed allocation
//! runs once per entry of its block: the loop rule (T-0117) holds by construction, and an
//! assignment's value, `s = String(…)` in a loop, keeps the heap.
//!
//! A copy of a placed value carries no region: copying out of a region builds on the heap
//! (T-0117 step 3, `List::copy` and `HashMap::copy`). Both steps rely on that library rule.
//! `return x` is never a copy, since C++ moves a local it returns.
//!
//! The reasons are what the editor and `dz placements` print after `region: ` and `heap: `,
//! so they are worded here, short and naming the program's own code.

use std::collections::{HashMap, HashSet};

use dz_ast::Dispatch;
use dz_check::{ItemSig, TypeTable};
use dz_hir::ty::Ty;
use dz_hir::{self as hir, BlockId, Body, BodyOwner, Callee, ExprId, Hir, LocalId, StmtId};
use dz_resolve::{Resolution, SymbolId};
use dz_span::{FileId, Span};

use crate::ties::{analyse_regions, Refusal, RegionRefusal};

/// Every body's sites, by the body's owner. A body with no site has no entry.
#[derive(Clone, Debug, Default)]
pub struct Placements {
    bodies: HashMap<BodyOwner, BodyPlacement>,
    /// The functions that take a hidden result placement ([`result_functions`]).
    results: HashSet<BodyOwner>,
}

impl Placements {
    pub fn body(&self, owner: &BodyOwner) -> Option<&BodyPlacement> {
        self.bodies.get(owner)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&BodyOwner, &BodyPlacement)> {
        self.bodies.iter()
    }

    /// Whether the function `owner` takes a hidden result placement: its declaration has
    /// one more parameter than it writes, and every call passes one (stage 2).
    pub fn takes_result(&self, owner: &BodyOwner) -> bool {
        self.results.contains(owner)
    }
}

/// The function a call calls, as a body's owner: what [`Placements::takes_result`] and
/// [`Placements::body`] are asked about. `None` for a call through a function value.
pub fn callee_owner(res: &Resolution, callee: &Callee) -> Option<(SymbolId, BodyOwner)> {
    Some(match callee {
        Callee::Fn { sym, .. } => (*sym, BodyOwner::Fn(*sym)),
        Callee::Ctor { owner, index } => (*owner, BodyOwner::Ctor { owner: *owner, index: *index }),
        Callee::Assoc { owner, name, .. } | Callee::BaseMethod { owner, name, .. } => {
            (*owner, BodyOwner::Method { owner: *owner, name: name.clone() })
        }
        Callee::Method { owner, name, .. } => {
            let o = if res.symbol(*owner).kind == dz_resolve::SymbolKind::Trait {
                BodyOwner::TraitDefaults { owner: *owner, name: name.clone() }
            } else {
                BodyOwner::Method { owner: *owner, name: name.clone() }
            };
            (*owner, o)
        }
        Callee::Value(_) => return None,
    })
}

/// Whether a call is `std`'s `Box(value)`, whose first argument becomes the box's own
/// contents: it lives where the box lives, as a struct literal's fields live where the
/// struct does (spec §14.6, "a `Box`'s contents go where the `Box` goes").
pub(crate) fn boxes_argument(res: &Resolution, callee: &Callee) -> bool {
    match callee {
        Callee::Ctor { owner, .. } => {
            let s = res.symbol(*owner);
            s.name == "Box" && s.module == res.prelude
        }
        _ => false,
    }
}

/// Whether `e` is a field of `self`, by fields alone: `m_items`, `self.a.b`. What a
/// constructor builds there lives where the value it makes is placed.
pub(crate) fn self_field(body: &Body, e: ExprId) -> bool {
    fn root(body: &Body, e: ExprId) -> bool {
        match body.expr(e).kind {
            hir::ExprKind::Local(l) => body.local(l).name == "self",
            hir::ExprKind::Deref(inner) => {
                matches!(body.expr(inner).kind, hir::ExprKind::Local(l) if body.local(l).name == "self")
            }
            hir::ExprKind::Field { base, .. } => root(body, base),
            _ => false,
        }
    }
    matches!(body.expr(e).kind, hir::ExprKind::Field { base, .. } if root(body, base))
}

/// The functions that take a hidden result placement (spec §14.6, T-0117 stage 2): each
/// whose result keeps an allocator ([`keeps_allocator`]), looking through `?`, decided
/// from signatures alone so that a declaration and every call agree:
///
/// - a free function, or a struct's or union's method that is neither `virtual` nor
///   `override`, that takes no allocator parameter of its own (a stage 1 site already says
///   where it builds);
/// - a trait's method, decided by the trait's signature, so every implementation (its
///   `ImplMethod` body, and the default `dz_mono` copies into it), the trait object's
///   vtable slot and a call through a bound all take one or none alike.
///
/// A function type takes one by the same rule ([`keeps_allocator`] on its result), so a
/// function value is called like a function; where the function it names takes none,
/// codegen adapts it. `virtual` methods are left out until a base and its overrides can be
/// decided together.
pub fn result_functions(hir: &Hir, table: &TypeTable, res: &Resolution) -> HashSet<BodyOwner> {
    let allocator = res.lookup(res.prelude, "Allocator");
    let holds = |t: &Ty| holds_allocator(table, allocator, t, &mut Vec::new());
    let mut out = HashSet::new();

    let mut traits: HashSet<SymbolId> =
        table.impls().iter().map(|i| i.trait_ref.trait_sym).collect();
    for (_, body) in hir.bodies() {
        if let BodyOwner::TraitDefaults { owner, .. } | BodyOwner::Method { owner, .. } =
            &body.owner
        {
            if res.symbol(*owner).kind == dz_resolve::SymbolKind::Trait {
                traits.insert(*owner);
            }
        }
    }
    for t in traits {
        let Some(s) = table.methods_sig(t) else { continue };
        for (name, sig) in &s.methods {
            if !holds(&sig.ret) {
                continue;
            }
            out.insert(BodyOwner::TraitDefaults { owner: t, name: name.clone() });
            out.insert(BodyOwner::Method { owner: t, name: name.clone() });
            for (idx, imp) in table.impls().iter().enumerate() {
                if imp.trait_ref.trait_sym == t {
                    out.insert(BodyOwner::ImplMethod { imp: idx as u32, name: name.clone() });
                }
            }
        }
    }

    for (_, body) in hir.bodies() {
        // A constructor builds into wherever its value is placed (round 3), when a field
        // keeps an allocator: it takes that placement and builds its fields there.
        if let BodyOwner::Ctor { owner, .. } = &body.owner {
            let fields = table.struct_sig(*owner).map(|s| s.fields.as_slice()).unwrap_or(&[]);
            if fields.iter().any(|f| holds(&f.ty)) && allocator_param(res, body).is_none() {
                out.insert(body.owner.clone());
            }
            continue;
        }
        let sig = match &body.owner {
            BodyOwner::Fn(sym) => match table.get(*sym) {
                Some(ItemSig::Fn(sig)) => sig,
                _ => continue,
            },
            BodyOwner::Method { owner, name }
                if res.symbol(*owner).kind != dz_resolve::SymbolKind::Trait =>
            {
                match table.methods_sig(*owner).and_then(|s| s.methods.get(name)) {
                    Some(sig) => sig,
                    None => continue,
                }
            }
            _ => continue,
        };
        if sig.dispatch != Dispatch::Static
            || !holds(&body.ret)
            || allocator_param(res, body).is_some()
        {
            continue;
        }
        out.insert(body.owner.clone());
    }
    out
}

/// Whether a function returning `ty` takes a result placement, and a function type with
/// that result passes one: `ty` keeps an allocator, through its fields, `?` or `!`.
pub fn keeps_allocator(table: &TypeTable, res: &Resolution, ty: &Ty) -> bool {
    holds_allocator(table, res.lookup(res.prelude, "Allocator"), ty, &mut Vec::new())
}

/// A function's signature by its body's owner, `self` not among its parameters.
fn owner_sig<'t>(table: &'t TypeTable, owner: &BodyOwner) -> Option<&'t dz_check::FnSig> {
    match owner {
        BodyOwner::Fn(sym) => match table.get(*sym)? {
            ItemSig::Fn(sig) => Some(sig),
            _ => None,
        },
        BodyOwner::Method { owner, name } | BodyOwner::TraitDefaults { owner, name } => {
            table.methods_sig(*owner)?.methods.get(name)
        }
        BodyOwner::ImplMethod { imp, name } => table.impls().get(*imp as usize)?.methods.get(name),
        BodyOwner::Ctor { owner, index } => table.struct_sig(*owner)?.ctors.get(*index),
        _ => None,
    }
}

/// Whether a value of `ty` keeps an allocator, in a field or a field's field: what a
/// placement can be handed. `List`, `String`, `HashMap` and what holds them do; a
/// `Formatter` or a `File` needs a drop but allocates nothing. A type parameter is taken
/// not to: a function returning a bare `T` hands back a value that exists already (`pop`,
/// `get`, `take`), and whether it takes a placement must not depend on the instance, since
/// every instance is called the same way. `seen` stops a type that holds itself through a
/// pointer.
fn holds_allocator(
    table: &TypeTable,
    allocator: Option<SymbolId>,
    ty: &Ty,
    seen: &mut Vec<Ty>,
) -> bool {
    match ty {
        Ty::Ptr { inner, .. } | Ty::OwnPtr { inner, .. } => {
            matches!(&**inner, Ty::Named { sym, .. } if Some(*sym) == allocator)
        }
        Ty::Optional(inner) | Ty::Fallible(inner) => holds_allocator(table, allocator, inner, seen),
        Ty::Array { elem, .. } => holds_allocator(table, allocator, elem, seen),
        Ty::Tuple(parts) => parts.iter().any(|t| holds_allocator(table, allocator, t, seen)),
        Ty::Named { sym, args } => {
            if seen.contains(ty) {
                return false;
            }
            seen.push(ty.clone());
            let fields = match table.get(*sym) {
                Some(ItemSig::Struct(s)) => table.field_types(*sym, args, &s.fields),
                Some(ItemSig::Union { variants, .. }) => {
                    variants.iter().flat_map(|v| table.payload_types(*sym, args, v)).collect()
                }
                _ => Vec::new(),
            };
            fields.iter().any(|t| holds_allocator(table, allocator, t, seen))
        }
        _ => false,
    }
}

/// The parameter of `body` that defaults to `Allocators::Global`, by index, `self` not
/// counted.
fn allocator_param(res: &Resolution, body: &Body) -> Option<usize> {
    body.param_defaults.iter().position(|d| {
        d.is_some_and(|d| {
            matches!(
                &body.expr(d).kind,
                hir::ExprKind::AssocConst { owner, name }
                    if name == "Global" && res.symbol(*owner).name == "Allocators"
            )
        })
    })
}

/// One body's sites, in source order.
#[derive(Clone, Debug)]
pub struct BodyPlacement {
    pub file: FileId,
    pub sites: Vec<Site>,
    /// The sites the proposer placed and the region check sent to the heap.
    demoted: Vec<ExprId>,
}

impl BodyPlacement {
    /// The blocks that declare a region, each once, in id order.
    pub fn region_blocks(&self) -> Vec<BlockId> {
        let mut blocks: Vec<BlockId> = self
            .sites
            .iter()
            .filter_map(|s| match s.decision {
                Decision::Region { block, .. } => Some(block),
                Decision::Result { .. } | Decision::Heap { .. } => None,
            })
            .collect();
        blocks.sort_by_key(|b| b.0);
        blocks.dedup();
        blocks
    }

    /// For a call placed in a region: its allocator argument's index in the callee's
    /// parameters (not counting `self`), and the region's block.
    pub fn region_for(&self, call: ExprId) -> Option<(usize, BlockId)> {
        match self.placed(call)? {
            (arg, Placed::Region(block)) => Some((arg, block)),
            (_, Placed::Result) => None,
        }
    }

    /// For a call placed anywhere but the heap: its allocator argument's index in the
    /// callee's parameters (not counting `self`; for a result function, the hidden one
    /// past the last it writes), and where the memory comes from.
    pub fn placed(&self, call: ExprId) -> Option<(usize, Placed)> {
        self.sites.iter().find(|s| s.call == call).and_then(|s| match s.decision {
            Decision::Region { block, .. } => Some((s.arg, Placed::Region(block))),
            Decision::Result { .. } => Some((s.arg, Placed::Result)),
            Decision::Heap { .. } => None,
        })
    }

    /// The calls the proposer placed and the region check refused.
    pub fn demoted(&self) -> &[ExprId] {
        &self.demoted
    }
}

/// One call that allocates where its allocator is left to its default.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Site {
    pub call: ExprId,
    pub span: Span,
    /// The callee as a reader names it: `List<Node>`, `String::from`.
    pub callee: String,
    /// The allocator argument's index in the callee's parameters, not counting `self`.
    pub arg: usize,
    pub decision: Decision,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decision {
    /// In the region of `block`, released when it ends. `scope` says which, for a reader:
    /// "`update`", "each pass of a loop in `main`".
    Region { block: BlockId, scope: String },
    /// Returned, so built wherever the function's caller keeps its result: the body's
    /// hidden result placement. `function` names the function, for a reader.
    Result { function: String },
    /// On the heap, and why; `at` is the use that decided it, where there is one.
    Heap { why: String, at: Option<Span> },
}

impl Decision {
    /// What the editor's hint and `dz placements` print: "region: `update`", "result:
    /// where `evens`' caller keeps it", "heap: stored in `self.items`". One spelling, so
    /// the two cannot disagree.
    pub fn label(&self) -> String {
        match self {
            Decision::Region { scope, .. } => format!("region: {scope}"),
            Decision::Result { function } => format!("result: where {function}'s caller keeps it"),
            Decision::Heap { why, .. } => format!("heap: {why}"),
        }
    }
}

/// Where a placed call's memory comes from, for codegen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placed {
    /// The region of this block.
    Region(BlockId),
    /// The body's own hidden result placement.
    Result,
}

/// Which proposals the region check is given.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Proposer {
    /// The proposer's rule: what [`place`] does.
    Rules,
    /// Every site in `std`'s placeable calls, in the region of the innermost block around
    /// it, so the region check alone decides. For testing the check: the fixed rules (`Rc`,
    /// the program's own functions, a formatting macro's body) still apply.
    Everything,
}

/// Decide every site in the program, body by body.
pub fn place(hir: &Hir, table: &TypeTable, res: &Resolution) -> Placements {
    place_with(hir, table, res, Proposer::Rules)
}

/// [`place`], with the proposals made by `proposer`.
pub fn place_with(
    hir: &Hir,
    table: &TypeTable,
    res: &Resolution,
    proposer: Proposer,
) -> Placements {
    let results = result_functions(hir, table, res);
    let mut out = Placements::default();
    for (_, body) in hir.bodies() {
        if let Some(placed) = place_body(hir, table, res, &results, body, proposer) {
            out.bodies.insert(body.owner.clone(), placed);
        }
    }
    out.results = results;
    out
}

/// Where a site is proposed to go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    /// The region of this block.
    Block(BlockId),
    /// The body's own result placement: the value is returned.
    Result,
}

/// Where a site is proposed to go, or why it keeps the heap and where that was decided.
type Proposal = Result<Target, (String, Option<Span>)>;

/// What a site's callee is, as far as placement cares.
struct Callable {
    /// The allocator parameter's index, not counting `self`.
    arg: usize,
    /// Declared in `std`.
    std: bool,
    /// A result function: its body builds only its result in the placement it is given,
    /// which the check holds it to, so the call can be placed whoever wrote it.
    result: bool,
    /// `Rc`, which a `Weak` can outlive (spec §14.6).
    rc: bool,
    name: String,
}

fn place_body(
    hir: &Hir,
    table: &TypeTable,
    res: &Resolution,
    results: &HashSet<BodyOwner>,
    body: &Body,
    proposer: Proposer,
) -> Option<BodyPlacement> {
    let mut found = Vec::new();
    for i in 0..body.exprs.len() {
        let e = ExprId(i as u32);
        if let Some(c) = callable(hir, table, res, results, body, e) {
            found.push((e, c));
        }
    }
    if found.is_empty() {
        return None;
    }
    found.sort_by_key(|(e, _)| (body.expr(*e).span.start, e.0));
    let cx = Cx::new(table, res, body, results.contains(&body.owner));

    // Step 1: what is proposed, and why the rest is not.
    let mut decided: Vec<(ExprId, Callable, Proposal)> = Vec::new();
    for (e, c) in found {
        let decision = cx.propose(e, &c, proposer);
        decided.push((e, c, decision));
    }

    // Step 2: the region check, over every proposal at once. A result placement outlives
    // the body: it is checked as a region of the body's outermost block that its end
    // does not release, and that a `return` may take.
    let proposals: Vec<(ExprId, BlockId, bool)> = decided
        .iter()
        .filter_map(|(e, _, d)| match d {
            Ok(Target::Block(b)) => Some((*e, *b, false)),
            Ok(Target::Result) => Some((*e, body.root, true)),
            Err(_) => None,
        })
        .collect();
    let refusals = if proposals.is_empty() {
        Vec::new()
    } else {
        analyse_regions(body, table, res, &proposals)
    };
    let refused: HashMap<ExprId, RegionRefusal> =
        proposals.iter().zip(refusals).filter_map(|((e, _, _), r)| r.map(|r| (*e, r))).collect();

    let mut sites = Vec::new();
    let mut demoted = Vec::new();
    for (e, c, d) in decided {
        let decision = match d {
            Ok(target) => match refused.get(&e) {
                Some(r) => {
                    demoted.push(e);
                    Decision::Heap { why: cx.refusal(&r.why), at: Some(r.at) }
                }
                None => match target {
                    Target::Block(block) => Decision::Region { block, scope: cx.scope(block) },
                    Target::Result => Decision::Result { function: cx.function() },
                },
            },
            Err((why, at)) => Decision::Heap { why, at },
        };
        sites.push(Site { call: e, span: body.expr(e).span, callee: c.name, arg: c.arg, decision });
    }
    Some(BodyPlacement { file: body.file, sites, demoted })
}

/// `e`, when it is a site: a call whose callee has a parameter defaulting to
/// `Allocators::Global` that the call leaves out, or a call to a result function.
fn callable(
    hir: &Hir,
    table: &TypeTable,
    res: &Resolution,
    results: &HashSet<BodyOwner>,
    body: &Body,
    e: ExprId,
) -> Option<Callable> {
    let node = body.expr(e);
    let hir::ExprKind::Call { callee, args } = &node.kind else { return None };
    let name = callee_name(res, body, callee, &node.ty);
    // A function value passes a placement when its type's result keeps an allocator: what
    // it calls is out of sight, but whatever it is takes one, adapted if need be.
    if let Callee::Value(v) = callee {
        let Ty::Fn { params, ret, .. } = body.ty(*v).peeled() else { return None };
        if !keeps_allocator(table, res, ret) {
            return None;
        }
        return Some(Callable { arg: params.len(), std: false, result: true, rc: false, name });
    }
    let (sym, owner) = callee_owner(res, callee)?;
    let s = res.symbol(sym);
    let std = s.module == res.prelude;
    if results.contains(&owner) {
        let arg = owner_sig(table, &owner).map(|sig| sig.params.len())?;
        return Some(Callable { arg, std, result: true, rc: false, name });
    }
    let callee_body = hir.find(&owner)?;
    let arg = allocator_param(res, callee_body)?;
    let left_out = match args.get(arg) {
        None => true,
        Some(a) => matches!(body.expr(*a).kind, hir::ExprKind::DefaultArg(_)),
    };
    if !left_out {
        return None;
    }
    Some(Callable {
        arg,
        std,
        result: false,
        rc: std && s.name == "Rc" && matches!(callee, Callee::Ctor { .. }),
        name,
    })
}

/// A callee as a reader names it.
fn callee_name(res: &Resolution, body: &Body, callee: &Callee, ty: &Ty) -> String {
    match callee {
        Callee::Fn { sym, .. } => res.symbol(*sym).name.clone(),
        Callee::Ctor { .. } => dz_check::type_name(res, ty),
        Callee::Assoc { owner, type_args, name, .. } => {
            let owner = Ty::Named { sym: *owner, args: type_args.clone() };
            format!("{}::{name}", dz_check::type_name(res, &owner))
        }
        Callee::Method { recv, name, .. } | Callee::BaseMethod { recv, name, .. } => {
            format!("{}.{name}", place_name(res, body, None, *recv))
        }
        Callee::Value(_) => "a function value".to_owned(),
    }
}

/// A place as the program writes it: `self.items`, `names[…]`.
fn place_name(res: &Resolution, body: &Body, table: Option<&TypeTable>, e: ExprId) -> String {
    match &body.expr(e).kind {
        hir::ExprKind::Local(l) => body.local(*l).name.clone(),
        hir::ExprKind::Field { base, index } => {
            let field = table.and_then(|t| match body.ty(*base).peeled_refs() {
                Ty::Named { sym, .. } => {
                    t.struct_sig(*sym).and_then(|s| s.fields.get(*index)).map(|f| f.name.clone())
                }
                _ => None,
            });
            let field = field.unwrap_or_else(|| index.to_string());
            format!("{}.{field}", place_name(res, body, table, *base))
        }
        hir::ExprKind::TupleIndex { base, index } => {
            format!("{}.{index}", place_name(res, body, table, *base))
        }
        hir::ExprKind::Index { base, .. } => format!("{}[…]", place_name(res, body, table, *base)),
        hir::ExprKind::Deref(inner) => place_name(res, body, table, *inner),
        hir::ExprKind::Static(sym) => res.symbol(*sym).name.clone(),
        // `xs[i]` on a container is a call to its `Index` or `IndexMut` (spec §11).
        hir::ExprKind::Call { callee: Callee::Method { recv, name, .. }, .. }
            if name == "index" || name == "index_mut" =>
        {
            format!("{}[…]", place_name(res, body, table, *recv))
        }
        hir::ExprKind::Call { callee: Callee::Method { recv, name, .. }, .. } => {
            format!("{}.{name}()", place_name(res, body, table, *recv))
        }
        _ => "…".to_owned(),
    }
}

/// How a block is entered, for naming its region.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Root,
    Loop,
    If,
    Case,
    Or,
    Plain,
    /// A formatting macro's body, which C++ gets as a lambda: no region is declared there.
    Lambda,
}

/// What is around a node.
#[derive(Clone, Copy)]
enum Up {
    Expr(ExprId),
    Stmt(StmtId),
}

struct Cx<'a> {
    table: &'a TypeTable,
    res: &'a Resolution,
    body: &'a Body,
    up: HashMap<ExprId, Up>,
    expr_block: HashMap<ExprId, BlockId>,
    stmt_block: HashMap<StmtId, BlockId>,
    /// Each block's kind, the block around it, and the expression it is part of.
    blocks: HashMap<BlockId, (Kind, Option<BlockId>, Option<ExprId>)>,
    /// The body is a result function's: what it returns can be built where its caller
    /// keeps it.
    returns_result: bool,
}

/// What the uses of a local do with it.
enum Escape {
    /// It stays where it is: borrowed, copied, assigned to.
    Stays,
    /// It is returned, and stays otherwise.
    Returned,
    /// It leaves, and why and where.
    Leaves((String, Option<Span>)),
}

impl<'a> Cx<'a> {
    fn new(
        table: &'a TypeTable,
        res: &'a Resolution,
        body: &'a Body,
        returns_result: bool,
    ) -> Self {
        let mut cx = Cx {
            table,
            res,
            body,
            returns_result,
            up: HashMap::new(),
            expr_block: HashMap::new(),
            stmt_block: HashMap::new(),
            blocks: HashMap::new(),
        };
        cx.walk_block(body.root, Kind::Root, None, None);
        cx
    }

    fn walk_block(&mut self, b: BlockId, kind: Kind, parent: Option<BlockId>, by: Option<ExprId>) {
        self.blocks.insert(b, (kind, parent, by));
        for s in self.body.block(b).stmts.clone() {
            self.walk_stmt(s, b);
        }
    }

    fn walk_stmt(&mut self, s: StmtId, b: BlockId) {
        self.stmt_block.insert(s, b);
        let up = Up::Stmt(s);
        match &self.body.stmt(s).kind {
            hir::StmtKind::Let { init, .. } => {
                if let Some(e) = init {
                    self.walk_expr(*e, up, b);
                }
            }
            hir::StmtKind::LetTuple { init, .. } => self.walk_expr(*init, up, b),
            hir::StmtKind::Assign { target, value, .. } => {
                self.walk_expr(*value, up, b);
                self.walk_expr(*target, up, b);
            }
            hir::StmtKind::Expr(e) => self.walk_expr(*e, up, b),
            hir::StmtKind::While { cond, body, .. } => {
                self.walk_expr(*cond, up, b);
                self.walk_block(*body, Kind::Loop, Some(b), None);
            }
            hir::StmtKind::For { subject, body, .. } => {
                self.walk_expr(*subject, up, b);
                self.walk_block(*body, Kind::Loop, Some(b), None);
            }
            hir::StmtKind::Defer(inner) => self.walk_stmt(*inner, b),
            hir::StmtKind::Item(_) | hir::StmtKind::Error => {}
        }
    }

    fn walk_expr(&mut self, e: ExprId, up: Up, b: BlockId) {
        self.up.insert(e, up);
        self.expr_block.insert(e, b);
        let here = Up::Expr(e);
        let kind = self.body.expr(e).kind.clone();
        let sub = |cx: &mut Self, x: ExprId| cx.walk_expr(x, here, b);
        match kind {
            hir::ExprKind::Field { base, .. }
            | hir::ExprKind::TupleIndex { base, .. }
            | hir::ExprKind::Len(base)
            | hir::ExprKind::DataPtr(base)
            | hir::ExprKind::Deref(base)
            | hir::ExprKind::Move(base)
            | hir::ExprKind::Ref { place: base, .. }
            | hir::ExprKind::Unary { operand: base, .. }
            | hir::ExprKind::Cast { operand: base, .. } => sub(self, base),
            hir::ExprKind::Index { base, index } => {
                sub(self, base);
                sub(self, index);
            }
            hir::ExprKind::SliceRange { base, start, end } => {
                sub(self, base);
                for x in [start, end].into_iter().flatten() {
                    sub(self, x);
                }
            }
            hir::ExprKind::Binary { lhs, rhs, .. } => {
                sub(self, lhs);
                sub(self, rhs);
            }
            hir::ExprKind::Range { start, end, .. } => {
                for x in [start, end].into_iter().flatten() {
                    sub(self, x);
                }
            }
            hir::ExprKind::StructLit { fields, .. }
            | hir::ExprKind::UnionVariant { payload: fields, .. } => {
                for x in fields.into_iter().flatten() {
                    sub(self, x);
                }
            }
            hir::ExprKind::Tuple(parts)
            | hir::ExprKind::ArrayLit(parts)
            | hir::ExprKind::Builtin { args: parts, .. }
            | hir::ExprKind::Intrinsic { args: parts, .. } => {
                for x in parts {
                    sub(self, x);
                }
            }
            hir::ExprKind::Call { callee, args } => {
                match callee {
                    Callee::Method { recv, .. } | Callee::BaseMethod { recv, .. } => {
                        sub(self, recv)
                    }
                    Callee::Value(v) => sub(self, v),
                    _ => {}
                }
                for x in args {
                    sub(self, x);
                }
            }
            hir::ExprKind::Block(block) => self.walk_block(block, Kind::Plain, Some(b), Some(e)),
            hir::ExprKind::If { cond, then, else_branch } => {
                sub(self, cond);
                self.walk_block(then, Kind::If, Some(b), Some(e));
                if let Some(other) = else_branch {
                    self.walk_block(other, Kind::If, Some(b), Some(e));
                }
            }
            hir::ExprKind::IfLet { operand, then, else_branch, .. } => {
                sub(self, operand);
                self.walk_block(then, Kind::If, Some(b), Some(e));
                if let Some(other) = else_branch {
                    self.walk_block(other, Kind::If, Some(b), Some(e));
                }
            }
            hir::ExprKind::Switch { subject, arms, default } => {
                sub(self, subject);
                for arm in arms {
                    for v in arm.values {
                        sub(self, v);
                    }
                    self.walk_block(arm.body, Kind::Case, Some(b), Some(e));
                }
                if let Some(d) = default {
                    self.walk_block(d, Kind::Case, Some(b), Some(e));
                }
            }
            hir::ExprKind::Guarded { init, value, .. } => {
                sub(self, init);
                sub(self, value);
            }
            hir::ExprKind::Formatted { args, body, .. } => {
                for x in args {
                    sub(self, x);
                }
                self.walk_block(body, Kind::Lambda, Some(b), Some(e));
            }
            hir::ExprKind::Or { operand, clause } => {
                sub(self, operand);
                match clause {
                    hir::OrClause::Value(v) => sub(self, v),
                    hir::OrClause::Block { body, .. } => {
                        self.walk_block(body, Kind::Or, Some(b), Some(e))
                    }
                    hir::OrClause::Ignore | hir::OrClause::Unreachable => {}
                }
            }
            hir::ExprKind::Return(Some(v)) => sub(self, v),
            _ => {}
        }
    }

    // --- the proposer -----------------------------------------------------------------

    /// Where `e` is proposed to go — a block's region, or the body's result placement when
    /// it is returned — or why it keeps the heap.
    fn propose(&self, e: ExprId, c: &Callable, proposer: Proposer) -> Proposal {
        if !c.std && !c.result {
            return Err((format!("made inside `{}`", c.name), None));
        }
        if c.rc {
            return Err(("a `Weak` can outlive the scope".to_owned(), None));
        }
        let what = match &self.body.owner {
            BodyOwner::Const(_) | BodyOwner::AssocConst { .. } => Some("a constant's value"),
            BodyOwner::Static(_) => Some("a static's value"),
            BodyOwner::FieldDefault { .. } => Some("a field's default"),
            _ => None,
        };
        if let Some(what) = what {
            return Err((what.to_owned(), None));
        }
        let Some(&block) = self.expr_block.get(&e) else {
            // A parameter's default or a base constructor's argument: evaluated by a
            // caller, or before the body.
            return Err(("not inside the body's blocks".to_owned(), None));
        };
        if self.blocks.get(&block).is_some_and(|(k, _, _)| *k == Kind::Lambda) {
            return Err(("inside a formatting macro".to_owned(), None));
        }
        if proposer == Proposer::Everything {
            return Ok(Target::Block(block));
        }
        // What holds the site's value — `make() or …`, `Store{ names: List() }`,
        // `Box(Node{ … })` — places it: a value's owned parts live where it lives.
        let top = self.holder(e);
        match self.up.get(&top) {
            Some(Up::Stmt(s)) => match &self.body.stmt(*s).kind {
                hir::StmtKind::Let { local: Some(x), init: Some(init) } if *init == top => {
                    let local = self.body.local(*x);
                    if local.ty != *self.body.ty(top) {
                        let ty = dz_check::type_name(self.res, &local.ty);
                        return Err((format!("kept as `{ty}`"), Some(local.span)));
                    }
                    let block = self.stmt_block.get(s).copied().unwrap_or(block);
                    match self.escape_of(*x, 0) {
                        Escape::Stays => Ok(Target::Block(block)),
                        Escape::Returned => Ok(Target::Result),
                        Escape::Leaves(found) => Err(found),
                    }
                }
                hir::StmtKind::Let { .. } => Err(("not kept by `let`".to_owned(), None)),
                hir::StmtKind::Assign { target, value, op, .. } if *value == top => {
                    // A constructor building a field of the value it makes: where that
                    // value is placed (round 3).
                    let ctor = matches!(self.body.owner, BodyOwner::Ctor { .. });
                    if ctor
                        && self.returns_result
                        && *op == hir::AssignOp::Assign
                        && self_field(self.body, *target)
                    {
                        return Ok(Target::Result);
                    }
                    let name = place_name(self.res, self.body, Some(self.table), *target);
                    let why = if self.in_loop(block) {
                        format!("assigned to `{name}` on every pass of a loop")
                    } else {
                        format!("assigned to `{name}`; only a `let` gets a region")
                    };
                    Err((why, Some(self.body.stmt(*s).span)))
                }
                _ => self.temporary(top),
            },
            _ => self.temporary(top),
        }
    }

    /// A value no `let` keeps: placed only when it is what the body returns.
    fn temporary(&self, e: ExprId) -> Proposal {
        match self.fate(e, 0) {
            Escape::Returned => Ok(Target::Result),
            Escape::Leaves(found) => Err(found),
            Escape::Stays => Err(("a temporary".to_owned(), None)),
        }
    }

    fn in_loop(&self, mut block: BlockId) -> bool {
        loop {
            match self.blocks.get(&block) {
                Some((Kind::Loop, _, _)) => return true,
                Some((_, Some(parent), _)) => block = *parent,
                _ => return false,
            }
        }
    }

    /// What the uses of `x` do with it: the first that takes it from where it is, in source
    /// order; else whether one returns it. `depth` bounds following one local into the
    /// next.
    fn escape_of(&self, x: LocalId, depth: u32) -> Escape {
        let mut uses: Vec<ExprId> = (0..self.body.exprs.len() as u32)
            .map(ExprId)
            .filter(|e| matches!(self.body.expr(*e).kind, hir::ExprKind::Local(l) if l == x))
            .collect();
        uses.sort_by_key(|e| (self.body.expr(*e).span.start, e.0));
        let mut returned = false;
        for u in uses {
            match self.fate(u, depth) {
                Escape::Stays => {}
                Escape::Returned => returned = true,
                Escape::Leaves(found) => return Escape::Leaves(found),
            }
        }
        if returned {
            Escape::Returned
        } else {
            Escape::Stays
        }
    }

    /// The outermost value holding `e`'s, through what moves it whole into another: a
    /// `move`, an `or` (its operand or its fallback), a struct or union literal, a tuple, an
    /// array, and the value a `Box` is made of.
    fn holder(&self, mut e: ExprId) -> ExprId {
        while let Some(Up::Expr(p)) = self.up.get(&e) {
            if self.holds(*p, e) {
                e = *p;
            } else {
                break;
            }
        }
        e
    }

    /// Whether `parent` holds `e`'s value as its own: what [`Self::holder`] climbs.
    fn holds(&self, parent: ExprId, e: ExprId) -> bool {
        match &self.body.expr(parent).kind {
            hir::ExprKind::Move(_)
            | hir::ExprKind::StructLit { .. }
            | hir::ExprKind::UnionVariant { .. }
            | hir::ExprKind::Tuple(_)
            | hir::ExprKind::ArrayLit(_) => true,
            hir::ExprKind::Or { operand, clause } => {
                *operand == e || matches!(clause, hir::OrClause::Value(v) if *v == e)
            }
            hir::ExprKind::Call { callee, args } => {
                args.first() == Some(&e) && boxes_argument(self.res, callee)
            }
            _ => false,
        }
    }

    /// Whether `e` reads a place by value without moving it: for a copyable type, a copy.
    fn is_copy(&self, e: ExprId) -> bool {
        matches!(
            self.body.expr(e).kind,
            hir::ExprKind::Local(_)
                | hir::ExprKind::Field { .. }
                | hir::ExprKind::TupleIndex { .. }
                | hir::ExprKind::Index { .. }
                | hir::ExprKind::Deref(_)
        ) && self.table.copyable(self.body.ty(e))
    }

    /// What becomes of `e`'s value, followed through whatever holds it ([`Self::holds`])
    /// and into a local a `let` moves it to: it stays where it is (borrowed, copied,
    /// assigned to, dropped with its block), it is returned, or it leaves, and why and
    /// where. A return is [`Escape::Returned`] only in a result function; elsewhere the
    /// value leaves with it. `depth` bounds following one local into the next.
    fn fate(&self, e: ExprId, depth: u32) -> Escape {
        let copy = self.is_copy(e);
        let span = |x: ExprId| Some(self.body.expr(x).span);
        let leaves = |why: String, at: Option<Span>| {
            if copy {
                Escape::Stays
            } else {
                Escape::Leaves((why, at))
            }
        };
        let Some(up) = self.up.get(&e) else {
            return Escape::Leaves(("used where this cannot follow it".to_owned(), span(e)));
        };
        match *up {
            Up::Stmt(s) => {
                let stmt = self.body.stmt(s);
                match &stmt.kind {
                    hir::StmtKind::Let { local: Some(y), init: Some(init) }
                        if *init == e && !copy && depth < 8 =>
                    {
                        // Moved into another local, which holds it from here on.
                        self.escape_of(*y, depth + 1)
                    }
                    hir::StmtKind::Let { local, .. } => {
                        let name =
                            local.map_or("_".to_owned(), |l| self.body.local(l).name.clone());
                        leaves(format!("moved to `{name}`"), Some(stmt.span))
                    }
                    hir::StmtKind::LetTuple { .. } => {
                        leaves("moved into a tuple".to_owned(), Some(stmt.span))
                    }
                    hir::StmtKind::Assign { target, .. } if *target == e => Escape::Stays,
                    hir::StmtKind::Assign { target, .. } => {
                        let name = place_name(self.res, self.body, Some(self.table), *target);
                        leaves(format!("stored in `{name}`"), Some(stmt.span))
                    }
                    hir::StmtKind::Expr(_) if self.is_tail_value(s) => {
                        Escape::Leaves(("the value of a block".to_owned(), span(e)))
                    }
                    hir::StmtKind::For { shape: hir::ForShape::Consuming, .. } => {
                        Escape::Leaves(("consumed by a `for`".to_owned(), Some(stmt.span)))
                    }
                    _ => Escape::Stays,
                }
            }
            Up::Expr(p) => {
                // `move(x)` takes `x` whole, copyable or not; anything else that holds a
                // copy holds a copy, which leaves `x` where it is.
                let moved = matches!(self.body.expr(p).kind, hir::ExprKind::Move(_));
                if (moved || !copy) && self.holds(p, e) {
                    return self.fate(p, depth);
                }
                let parent = self.body.expr(p);
                match &parent.kind {
                    hir::ExprKind::Ref { .. }
                    | hir::ExprKind::Len(_)
                    | hir::ExprKind::DataPtr(_)
                    | hir::ExprKind::Deref(_)
                    | hir::ExprKind::Switch { .. }
                    | hir::ExprKind::If { .. } => Escape::Stays,
                    hir::ExprKind::Index { base, .. } | hir::ExprKind::SliceRange { base, .. }
                        if *base == e =>
                    {
                        Escape::Stays
                    }
                    // A field read follows the field: a copy of it leaves the value alone.
                    hir::ExprKind::Field { .. } | hir::ExprKind::TupleIndex { .. } => {
                        self.fate(p, depth)
                    }
                    hir::ExprKind::Call { callee, .. } => match callee {
                        // Every Dazzle method borrows its receiver (spec §7.3).
                        Callee::Method { recv, .. } | Callee::BaseMethod { recv, .. }
                            if *recv == e =>
                        {
                            Escape::Stays
                        }
                        _ => leaves(format!("passed to `{}`", self.call_name(p)), span(p)),
                    },
                    hir::ExprKind::Return(_) => {
                        // C++ moves a local it returns, so that is never a copy.
                        let moved = matches!(self.body.expr(e).kind, hir::ExprKind::Local(_));
                        if copy && !moved {
                            Escape::Stays
                        } else if self.returns_result {
                            Escape::Returned
                        } else {
                            Escape::Leaves(("returned".to_owned(), span(p)))
                        }
                    }
                    hir::ExprKind::IfLet { binding, .. } => {
                        if matches!(self.body.local(*binding).ty, Ty::Ref { .. }) {
                            Escape::Stays
                        } else {
                            leaves("taken out by `if let`".to_owned(), span(p))
                        }
                    }
                    hir::ExprKind::Or { .. } => leaves("taken out by `or`".to_owned(), span(p)),
                    hir::ExprKind::Formatted { .. } => {
                        leaves("passed to a formatting macro".to_owned(), span(p))
                    }
                    hir::ExprKind::Builtin { name, args }
                    | hir::ExprKind::Intrinsic { name, args } => {
                        let in_place = args.first() == Some(&e)
                            && matches!(name.as_str(), "construct" | "destroy" | "move_out");
                        if in_place {
                            Escape::Stays
                        } else {
                            leaves(format!("passed to `{name}`"), span(p))
                        }
                    }
                    _ => leaves("used by an operator".to_owned(), span(p)),
                }
            }
        }
    }

    /// Whether `s` is the last statement of a block whose value is used.
    fn is_tail_value(&self, s: StmtId) -> bool {
        let Some(b) = self.stmt_block.get(&s) else { return false };
        if self.body.block(*b).stmts.last() != Some(&s) {
            return false;
        }
        match self.blocks.get(b) {
            Some((_, _, Some(by))) => {
                !matches!(self.body.ty(*by), Ty::Void | Ty::Never | Ty::Error)
            }
            _ => false,
        }
    }

    // --- words --------------------------------------------------------------------------

    fn call_name(&self, call: ExprId) -> String {
        match &self.body.expr(call).kind {
            hir::ExprKind::Call { callee, .. } => match callee {
                Callee::Method { recv, name, .. } | Callee::BaseMethod { recv, name, .. } => {
                    format!("{}.{name}", place_name(self.res, self.body, Some(self.table), *recv))
                }
                _ => callee_name(self.res, self.body, callee, self.body.ty(call)),
            },
            hir::ExprKind::Builtin { name, .. } | hir::ExprKind::Intrinsic { name, .. } => {
                name.clone()
            }
            _ => "…".to_owned(),
        }
    }

    /// The function the body is, as `region: ` names it.
    fn function(&self) -> String {
        let sym = |s: &SymbolId| self.res.symbol(*s).name.clone();
        match &self.body.owner {
            BodyOwner::Fn(s) => format!("`{}`", sym(s)),
            BodyOwner::Method { name, .. }
            | BodyOwner::TraitDefaults { name, .. }
            | BodyOwner::ImplMethod { name, .. } => format!("`{name}`"),
            BodyOwner::Ctor { owner, .. } => format!("the `{}` constructor", sym(owner)),
            BodyOwner::Copy(owner) => format!("`{}`'s copy", sym(owner)),
            BodyOwner::Drop(owner) => format!("`{}`'s drop", sym(owner)),
            BodyOwner::Const(s) | BodyOwner::Static(s) => format!("`{}`", sym(s)),
            BodyOwner::AssocConst { name, .. } => format!("`{name}`"),
            BodyOwner::FieldDefault { owner, .. } => format!("`{}`", sym(owner)),
        }
    }

    /// Which region a block's is, for a reader.
    fn scope(&self, block: BlockId) -> String {
        let f = self.function();
        match self.blocks.get(&block).map(|(k, _, _)| *k) {
            Some(Kind::Root) | None => f,
            Some(Kind::Loop) => format!("each pass of a loop in {f}"),
            Some(Kind::If) => format!("an `if` in {f}"),
            Some(Kind::Case) => format!("a `case` in {f}"),
            Some(Kind::Or) => format!("an `or` block in {f}"),
            Some(Kind::Plain) | Some(Kind::Lambda) => format!("a block in {f}"),
        }
    }

    fn refusal(&self, why: &Refusal) -> String {
        match why {
            Refusal::Returned => "returned".to_owned(),
            Refusal::Stored(target) => {
                format!(
                    "stored in `{}`",
                    place_name(self.res, self.body, Some(self.table), *target)
                )
            }
            Refusal::Passed(call) => match self.body.expr(*call).kind {
                hir::ExprKind::Formatted { .. } => "passed to a formatting macro".to_owned(),
                _ => format!("passed to `{}`", self.call_name(*call)),
            },
            Refusal::Outlives(block) => format!("outlives {}", self.scope(*block)),
            Refusal::EveryPass => "made on every pass of a loop".to_owned(),
            Refusal::Unfollowed(what) => (*what).to_owned(),
        }
    }
}
