//! Owner arenas (spec §14.6, T-0120): which containers are emptied only all at once, so
//! that what is stored into them can be built in an arena their owner keeps, reset when
//! the container is cleared.
//!
//! The first version covers the case `frame` measures: a private `List` field of a struct
//! that is not generic, holding values that own memory. The struct's own bodies are all
//! that can reach a private field, so they are all that is read. The field gets an arena
//! when every use of it in them is one of:
//!
//! - a `&self` method (`len`, `iter`, what `for &c in …` calls): nothing leaves;
//! - a shared borrow, `&m_commands`;
//! - `push`;
//! - `clear`, which ends everything in it at once.
//!
//! Anything else — `pop`, `remove`, `truncate`, an element assigned or indexed, the field
//! lent `&mut`, moved or assigned whole — could take an element out or end it alone, and
//! the field keeps no arena.
//!
//! An arena is reset when the container is cleared, so nothing in it may be alive then
//! outside the container. Inside the struct, that is two more rules: a body that clears
//! the field takes no by-value parameter that owns memory, which could be in the arena;
//! and a body that pushes a parameter into it neither clears it nor calls anything on
//! `self`, which could. Outside, the caller's side is the proposer's
//! ([`crate::placement`]): the container is not used between building the value and
//! handing it over.

use std::collections::HashMap;

use dz_check::TypeTable;
use dz_hir::ty::Ty;
use dz_hir::{self as hir, Body, BodyOwner, Callee, ExprId, Hir, LocalId};
use dz_resolve::{Resolution, SymbolId};

/// The fields that keep an owner arena, by the struct that owns them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OwnerArenas {
    fields: HashMap<SymbolId, Vec<ArenaField>>,
}

/// One field that keeps an owner arena.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArenaField {
    /// The field's index in the struct's flat field list, and its name.
    pub index: usize,
    pub name: String,
    /// The methods that push one of their by-value parameters into the field, by name,
    /// each with those parameters' indexes, not counting `self`: what a caller builds in
    /// the arena when it passes them.
    pushers: HashMap<String, Vec<usize>>,
}

impl OwnerArenas {
    /// The fields of `owner` that keep an arena, in field order.
    pub fn of(&self, owner: SymbolId) -> &[ArenaField] {
        self.fields.get(&owner).map(Vec::as_slice).unwrap_or(&[])
    }

    /// The field of `owner` whose arena an argument `arg` of a call to `method` is stored
    /// into, when there is one.
    pub fn stored_by(&self, owner: SymbolId, method: &str, arg: usize) -> Option<&ArenaField> {
        self.of(owner)
            .iter()
            .find(|f| f.pushers.get(method).is_some_and(|args| args.contains(&arg)))
    }

    /// Whether `e`, in `body`, is a field of `self` that keeps an arena: the receiver of a
    /// `clear` that resets it. The field's name when it is.
    pub fn field_of_self(&self, body: &Body, e: ExprId) -> Option<&ArenaField> {
        let hir::ExprKind::Field { base, index } = body.expr(e).kind else { return None };
        if !is_self(body, base) {
            return None;
        }
        let Ty::Named { sym, .. } = body.ty(base).peeled_refs() else { return None };
        self.of(*sym).iter().find(|f| f.index == index)
    }
}

/// Decide which fields of the program's structs keep an owner arena.
pub fn owner_arenas(hir: &Hir, table: &TypeTable, res: &Resolution) -> OwnerArenas {
    let mut candidates: HashMap<(SymbolId, usize), Candidate> = HashMap::new();
    for (_, body) in hir.bodies() {
        let Some(owner) = struct_of(&body.owner) else { continue };
        if table.is_generic(owner) {
            continue;
        }
        let Some(sig) = table.struct_sig(owner) else { continue };
        for (index, f) in sig.fields.iter().enumerate() {
            if f.vis != dz_ast::Vis::Private || f.declared_in.is_some() {
                continue;
            }
            if !holds_owning_list(table, res, &f.ty) {
                continue;
            }
            candidates.entry((owner, index)).or_insert_with(|| Candidate {
                name: f.name.clone(),
                ok: true,
                pushers: HashMap::new(),
            });
        }
    }
    if candidates.is_empty() {
        return OwnerArenas::default();
    }

    for (_, body) in hir.bodies() {
        let own = struct_of(&body.owner);
        for i in 0..body.exprs.len() as u32 {
            let e = ExprId(i);
            let hir::ExprKind::Field { base, index } = body.expr(e).kind else { continue };
            let Ty::Named { sym, .. } = body.ty(base).peeled_refs() else { continue };
            let Some(c) = candidates.get_mut(&(*sym, index)) else { continue };
            // A field reached from anywhere but its own struct's bodies, or not through
            // `self` in them, is not one the struct alone decides about.
            if own != Some(*sym) || !is_self(body, base) {
                c.ok = false;
            }
        }
        let Some(owner) = own else { continue };
        let uses = Uses::of(table, body, owner, &candidates);
        for ((sym, index), use_) in uses.0 {
            let Some(c) = candidates.get_mut(&(sym, index)) else { continue };
            match use_ {
                Use::Refused => c.ok = false,
                Use::Allowed { pushed, clears } => {
                    if clears && owns_by_value(table, body) {
                        c.ok = false;
                    }
                    if !pushed.is_empty() {
                        if clears || touches_self(body) {
                            c.ok = false;
                        }
                        if let BodyOwner::Method { name, .. } = &body.owner {
                            c.pushers.entry(name.clone()).or_default().extend(pushed);
                        } else {
                            // A constructor pushing what it is given: no call names it.
                            c.ok = false;
                        }
                    }
                }
            }
        }
    }

    let mut out = OwnerArenas::default();
    for ((owner, index), c) in candidates {
        if c.ok {
            out.fields.entry(owner).or_default().push(ArenaField {
                index,
                name: c.name,
                pushers: c.pushers,
            });
        }
    }
    for fields in out.fields.values_mut() {
        fields.sort_by_key(|f| f.index);
    }
    out
}

struct Candidate {
    name: String,
    ok: bool,
    pushers: HashMap<String, Vec<usize>>,
}

/// How one body uses each candidate field of its own struct.
struct Uses(HashMap<(SymbolId, usize), Use>);

enum Use {
    /// Only as an arena allows: the by-value parameters it pushes, by index not counting
    /// `self`, and whether it clears.
    Allowed {
        pushed: Vec<usize>,
        clears: bool,
    },
    Refused,
}

impl Uses {
    fn of(
        table: &TypeTable,
        body: &Body,
        owner: SymbolId,
        candidates: &HashMap<(SymbolId, usize), Candidate>,
    ) -> Self {
        let mut out: HashMap<(SymbolId, usize), Use> = HashMap::new();
        // Every use of a candidate field of `self`, and which of them an arena allows.
        let mut fields: Vec<(ExprId, usize)> = Vec::new();
        for i in 0..body.exprs.len() as u32 {
            let e = ExprId(i);
            if let hir::ExprKind::Field { base, index } = body.expr(e).kind {
                if is_self(body, base) && candidates.contains_key(&(owner, index)) {
                    fields.push((e, index));
                }
            }
        }
        if fields.is_empty() {
            return Uses(out);
        }
        let mut allowed: HashMap<ExprId, Option<usize>> = HashMap::new();
        let mut clears: Vec<usize> = Vec::new();
        for i in 0..body.exprs.len() as u32 {
            let e = ExprId(i);
            match &body.expr(e).kind {
                hir::ExprKind::Ref { mutable: false, place, .. } => {
                    allowed.insert(*place, None);
                }
                hir::ExprKind::Call {
                    callee: Callee::Method { recv, owner: m, name, .. },
                    args,
                } => {
                    let Some(&(_, index)) = fields.iter().find(|(f, _)| f == recv) else {
                        continue;
                    };
                    let shared = table
                        .methods_sig(*m)
                        .and_then(|s| s.methods.get(name))
                        .is_some_and(|sig| sig.takes_self == Some(false));
                    if shared {
                        allowed.insert(*recv, None);
                    } else if name == "clear" && args.is_empty() {
                        allowed.insert(*recv, None);
                        clears.push(index);
                    } else if name == "push" {
                        allowed.insert(*recv, args.first().and_then(|a| param_of(body, *a)));
                    }
                }
                _ => {}
            }
        }
        for (e, index) in fields {
            let entry = out
                .entry((owner, index))
                .or_insert(Use::Allowed { pushed: Vec::new(), clears: false });
            match (allowed.get(&e), &mut *entry) {
                (Some(pushed), Use::Allowed { pushed: list, .. }) => list.extend(*pushed),
                (None, _) => *entry = Use::Refused,
                (Some(_), Use::Refused) => {}
            }
        }
        for index in clears {
            if let Some(Use::Allowed { clears, .. }) = out.get_mut(&(owner, index)) {
                *clears = true;
            }
        }
        Uses(out)
    }
}

/// The struct whose own body this is: a method, constructor, `copy` or drop.
fn struct_of(owner: &BodyOwner) -> Option<SymbolId> {
    match owner {
        BodyOwner::Method { owner, .. }
        | BodyOwner::Ctor { owner, .. }
        | BodyOwner::Copy(owner)
        | BodyOwner::Drop(owner) => Some(*owner),
        _ => None,
    }
}

/// Whether `e` is `self`, or `*self`.
fn is_self(body: &Body, e: ExprId) -> bool {
    match body.expr(e).kind {
        hir::ExprKind::Local(l) => body.local(l).name == "self",
        hir::ExprKind::Deref(inner) => {
            matches!(body.expr(inner).kind, hir::ExprKind::Local(l) if body.local(l).name == "self")
        }
        _ => false,
    }
}

/// `std`'s `List<E>`, with `E` owning memory: what is worth an arena.
fn holds_owning_list(table: &TypeTable, res: &Resolution, ty: &Ty) -> bool {
    let Ty::Named { sym, args } = ty else { return false };
    let s = res.symbol(*sym);
    s.name == "List"
        && s.module == res.prelude
        && args.first().is_some_and(|e| crate::storing::owns(table, e))
}

/// The index, not counting `self`, of the by-value parameter `e` is, or moves.
fn param_of(body: &Body, e: ExprId) -> Option<usize> {
    let e = match body.expr(e).kind {
        hir::ExprKind::Move(inner) => inner,
        _ => e,
    };
    let hir::ExprKind::Local(l) = body.expr(e).kind else { return None };
    by_value_params(body).iter().position(|p| *p == l)
}

/// The body's parameters, `self` left out.
fn by_value_params(body: &Body) -> Vec<LocalId> {
    body.params.iter().copied().filter(|p| body.local(*p).name != "self").collect()
}

/// Whether the body takes a by-value parameter that owns memory, which a caller may have
/// built in the arena.
fn owns_by_value(table: &TypeTable, body: &Body) -> bool {
    by_value_params(body).iter().any(|p| {
        let ty = &body.local(*p).ty;
        !matches!(ty, Ty::Ref { .. } | Ty::Ptr { .. }) && crate::storing::owns(table, ty)
    })
}

/// Whether `self` is used other than to reach a field: a method called on it, or it lent.
fn touches_self(body: &Body) -> bool {
    (0..body.exprs.len() as u32).map(ExprId).any(|e| {
        if !is_self(body, e) || matches!(body.expr(e).kind, hir::ExprKind::Deref(_)) {
            return false;
        }
        // `self` itself: fine only as the base of a field.
        !(0..body.exprs.len() as u32).map(ExprId).any(|p| {
            matches!(body.expr(p).kind, hir::ExprKind::Field { base, .. } if base == e)
                || matches!(body.expr(p).kind, hir::ExprKind::Deref(inner) if inner == e)
        }) || (0..body.exprs.len() as u32).map(ExprId).any(|p| {
            matches!(&body.expr(p).kind,
                hir::ExprKind::Call { callee: Callee::Method { recv, .. }, .. } if *recv == e)
                || matches!(&body.expr(p).kind, hir::ExprKind::Call { args, .. } if args.contains(&e))
                || matches!(body.expr(p).kind, hir::ExprKind::Ref { place, .. } if place == e)
        })
    })
}
