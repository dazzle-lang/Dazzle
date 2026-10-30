//! Call effects for the tie analysis (spec §6.7, T-0107 step 2c).
//!
//! The checker resolves names; this module gives those names a common representation
//! and substitutes a call's arguments. In particular, `self@text` and `self.m_bytes`
//! name the same field, but `xs@items` and `xs[*]@text` must stay separate: moving an
//! array does not move the allocations its elements own.
//!
//! [`analyse_body`] follows origins and invalidations through a checked body. Its
//! findings are kept separate from [`crate::check`]'s diagnostics until std's clauses,
//! diagnostics and fixes are ready to enable together (T-0107 steps 2d/2e).

mod body;
mod report;
pub use body::{
    analyse_body, BodyAnalysis, BodyKill, Incomplete, InvalidReturn, InvalidUse, KillKind,
};
pub use report::report;

use dz_check::ties::{self, Root, Scope};
use dz_check::TypeTable;
use dz_diag::codes;
use dz_hir::ty::Ty;
use dz_hir::LocalId;
use dz_span::Span;

/// A place in the caller, with no public/private aliases left in its path.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Place {
    pub local: LocalId,
    pub steps: Vec<Step>,
}

/// Canonical path steps. Indices deliberately have no value: `xs[i]` and `xs[j]`
/// may refer to the same element (spec §6.7).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Step {
    Field(String),
    Elements,
    /// The allocation behind a pointer field, as distinct from the field holding the
    /// pointer. Only a declared tie introduces this step; raw pointer fields do not
    /// acquire checked provenance by being read.
    Storage,
}

impl Place {
    pub fn local(local: LocalId) -> Self {
        Self { local, steps: Vec::new() }
    }

    /// Whether ending `self` ends `other` too. This is directional: killing a field
    /// does not end a reference to its enclosing struct.
    pub fn covers(&self, other: &Self) -> bool {
        self.local == other.local && other.steps.starts_with(&self.steps)
    }

    fn followed_by(&self, steps: &[Step]) -> Self {
        let mut place = self.clone();
        place.steps.extend_from_slice(steps);
        place
    }
}

/// The caller's origins for the receiver and each formal parameter. These are origins
/// of the referenced values, not the locals holding the references. A borrow or alias
/// of `world.enemies` supplies that same place, regardless of the argument's spelling.
///
/// `None` explicitly means unknown provenance (for example a raw pointer loaded from a
/// field), not "no effect". An omitted default argument may also be unknown.
#[derive(Clone, Debug, Default)]
pub struct Bindings {
    pub receiver: Option<Place>,
    /// Does not count `self`; the checker's parameter order, including named arguments.
    pub params: Vec<Option<Place>>,
}

impl Bindings {
    fn get(&self, root: Root) -> Option<&Place> {
        match root {
            Root::SelfValue => self.receiver.as_ref(),
            Root::Param(i) => self.params.get(i).and_then(Option::as_ref),
        }
    }
}

/// A canonical path still rooted in the callee's signature. Keeping the root symbolic
/// allows the same normalized signature to be reused at every call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Path {
    pub root: Root,
    pub steps: Vec<Step>,
}

impl Path {
    pub fn bind(&self, bindings: &Bindings) -> Option<Place> {
        Some(bindings.get(self.root)?.followed_by(&self.steps))
    }
}

/// Normalize a checked tie path. A tie stands for the first path its declaration lists,
/// and every other path it lists is rewritten to that one, as is every path `[*]` stands
/// for to `[*]`: a tie is one span of validity (spec §6.7). A path ending at a pointer
/// names the storage behind it.
pub fn normalize(scope: &Scope<'_>, path: &dz_ast::TiePath) -> Result<Path, ties::Error> {
    let resolved = ties::resolve(scope, path)?;
    let root = match resolved.root {
        Root::SelfValue => scope.self_ty.clone().unwrap_or(Ty::Error),
        Root::Param(i) => scope.sig.params[i].ty.clone(),
    };
    let (steps, _) = canonical_steps(scope.table, scope.res, root.clone(), &resolved.steps)
        .ok_or_else(|| internal_path_error(path.span))?;
    let steps = canonicalize(scope.table, scope.res, &root, &steps);
    Ok(Path { root: resolved.root, steps })
}

/// Resolved `steps` from a value of type `ty`, as canonical steps, and the type they end
/// at. A field reads as the first field sharing its tie, a tie as what its first path
/// names, and a pointer field before `[*]` as the storage it points at.
fn canonical_steps(
    table: &TypeTable,
    res: &dz_resolve::Resolution,
    mut ty: Ty,
    steps: &[ties::Step],
) -> Option<(Vec<Step>, Ty)> {
    let mut out = Vec::new();
    for (i, step) in steps.iter().enumerate() {
        match step {
            ties::Step::Field(name) => {
                let canonical = match ty.peeled() {
                    Ty::Named { sym, .. } => canonical_field(table, *sym, name),
                    _ => name.clone(),
                };
                let next = field_of(table, &ty, name)?;
                out.push(Step::Field(canonical));
                if is_pointer(&next) && steps.get(i + 1) == Some(&ties::Step::Elements) {
                    out.push(Step::Storage);
                }
                ty = next;
            }
            ties::Step::Tie(name) => {
                let (inner, end) = tie_steps(table, res, &ty, name)?;
                out.extend(inner);
                ty = end;
            }
            ties::Step::Elements => {
                ty = match &ty {
                    Ty::Ptr { inner, .. } | Ty::OwnPtr { inner, .. }
                        if out.last() == Some(&Step::Storage) =>
                    {
                        (**inner).clone()
                    }
                    // An Iterable-only path ends here; there is no element type to follow.
                    _ => ties::elements_in(table, &ty).flatten().unwrap_or(Ty::Error),
                };
                out.push(Step::Elements);
            }
        }
    }
    Some((out, ty))
}

/// What `ty`'s tie `@name` names, as canonical steps from `ty`: its declaration's first
/// path, with the storage behind a pointer it ends at.
fn tie_steps(
    table: &TypeTable,
    res: &dz_resolve::Resolution,
    ty: &Ty,
    name: &str,
) -> Option<(Vec<Step>, Ty)> {
    let Ty::Named { sym, args } = ty.peeled() else { return None };
    let decl =
        ties::tie_decls(table, *sym).into_iter().find(|t| t.name.as_deref() == Some(name))?;
    export_steps(table, res, *sym, args, decl.paths.first()?)
}

/// One path of one of `sym`'s `tie` declarations, as canonical steps from the type.
fn export_steps(
    table: &TypeTable,
    res: &dz_resolve::Resolution,
    sym: dz_resolve::SymbolId,
    args: &[Ty],
    path: &dz_ast::TiePath,
) -> Option<(Vec<Step>, Ty)> {
    let (inner, _, _) = ties::export_path(table, res, sym, args, path).ok()?;
    let (mut steps, end) =
        canonical_steps(table, res, Ty::Named { sym, args: args.to_vec() }, &inner)?;
    if is_pointer(&end) && steps.last() != Some(&Step::Storage) {
        steps.push(Step::Storage);
    }
    Some((steps, end))
}

fn is_pointer(ty: &Ty) -> bool {
    matches!(ty, Ty::Ptr { .. } | Ty::OwnPtr { .. })
}

/// What `sym`'s ties rewrite, at `args`: a path the type's tie stands for, other than
/// the first, becomes the first, and a path `[*]` stands for becomes `[*]`. Each with
/// the type the replacement ends at.
fn rewrites(
    table: &TypeTable,
    res: &dz_resolve::Resolution,
    sym: dz_resolve::SymbolId,
    args: &[Ty],
) -> Vec<(Vec<Step>, Vec<Step>, Ty)> {
    let mut out = Vec::new();
    for decl in ties::tie_decls(table, sym) {
        let paths: Vec<_> =
            decl.paths.iter().filter_map(|p| export_steps(table, res, sym, args, p)).collect();
        match decl.name {
            Some(_) => {
                let Some((first, first_ty)) = paths.first() else { continue };
                for (p, _) in &paths[1..] {
                    if p != first {
                        out.push((p.clone(), first.clone(), first_ty.clone()));
                    }
                }
            }
            None => {
                let this = Ty::Named { sym, args: args.to_vec() };
                let elem = ties::elements_in(table, &this).flatten().unwrap_or(Ty::Error);
                for (p, _) in paths {
                    out.push((p, vec![Step::Elements], elem.clone()));
                }
            }
        }
    }
    out
}

/// `steps` from a value of type `root`, with every path a tie stands for rewritten to the
/// one place that names it ([`rewrites`]), so that two spellings of one tie compare equal.
pub(crate) fn canonicalize(
    table: &TypeTable,
    res: &dz_resolve::Resolution,
    root: &Ty,
    steps: &[Step],
) -> Vec<Step> {
    let mut out = Vec::with_capacity(steps.len());
    let mut ty = root.peeled().clone();
    let mut i = 0;
    'outer: while i < steps.len() {
        if let Ty::Named { sym, args } = ty.peeled_refs() {
            if !ties::tie_decls(table, *sym).is_empty() {
                for (pattern, replacement, after) in rewrites(table, res, *sym, args) {
                    if steps[i..].starts_with(&pattern) {
                        out.extend(replacement);
                        ty = after;
                        i += pattern.len();
                        continue 'outer;
                    }
                }
            }
        }
        let step = &steps[i];
        ty = match step {
            Step::Field(name) => field_of(table, &ty, name).unwrap_or(Ty::Error),
            // The pointer stays the type until `[*]` reads what it points at.
            Step::Storage => ty,
            Step::Elements => match &ty {
                Ty::Ptr { inner, .. } | Ty::OwnPtr { inner, .. } => (**inner).clone(),
                _ => ties::elements_in(table, &ty).flatten().unwrap_or(Ty::Error),
            },
        };
        out.push(step.clone());
        i += 1;
    }
    out
}

/// [`canonicalize`] for a place, whose root is its local of type `root`.
pub(crate) fn canonical_place(
    table: &TypeTable,
    res: &dz_resolve::Resolution,
    root: &Ty,
    place: &Place,
) -> Place {
    Place { local: place.local, steps: canonicalize(table, res, root, &place.steps) }
}

/// The field standing for `name` in `sym`'s places. Fields tied together are one span of
/// validity (spec §6.7), so all of them are the first field declaring their tie: a kill
/// through `m_keys` ends a view into `m_values`.
pub(crate) fn canonical_field(table: &TypeTable, sym: dz_resolve::SymbolId, name: &str) -> String {
    let Some(s) = table.struct_sig(sym) else { return name.to_owned() };
    let tie = s.fields.iter().find(|f| f.name == name).and_then(|f| f.tie.as_ref());
    match tie.and_then(|t| s.fields.iter().find(|f| f.tie.as_ref() == Some(t))) {
        Some(first) => first.name.clone(),
        None => name.to_owned(),
    }
}

fn field(scope: &Scope<'_>, ty: &Ty, name: &str) -> Option<Ty> {
    field_of(scope.table, ty, name)
}

fn field_of(table: &TypeTable, ty: &Ty, name: &str) -> Option<Ty> {
    let Ty::Named { sym, args } = ty.peeled() else { return None };
    let f = table.struct_sig(*sym)?.fields.iter().find(|f| f.name == name)?;
    Some(table.field_types(*sym, args, std::slice::from_ref(f)).remove(0))
}

fn internal_path_error(span: Span) -> ties::Error {
    // Resolution normally makes this impossible. Keep error recovery total when the
    // editor supplies an incomplete signature/table rather than panicking in flow.
    ties::Error {
        code: codes::INTERNAL_ERROR,
        span,
        message: "a resolved tie path could not be normalized".into(),
        label: "the signature and its type information disagree".into(),
        help: "this is a compiler bug; please report the source that caused it".into(),
    }
}

/// A kill in caller coordinates. The clause's span stays in the callee's source; a
/// diagnostic at the call must use the call's own span as its primary label.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Kill {
    pub place: Place,
    pub clause: Option<Span>,
}

/// Keep elision distinct so diagnostics can explain why a result is tied to an input
/// whole rather than the narrower memory an explicit annotation could name (§6.7).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReturnTie {
    /// `in a` or `in (a, b)`: the result may point into any of them.
    Explicit(Vec<Place>),
    Elided(Place),
    /// No reference-like result.
    None,
    /// A reference-like result with no uniquely known input, an aggregate holding a
    /// view, or unknown provenance.
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParameterTie {
    Unspecified,
    Known(Place),
    Unknown,
}

impl ReturnTie {
    pub fn ended_by(&self, killed: &Place) -> bool {
        // An `in` path ending at a value means all ties beneath that value, so a kill
        // inside it may end this view as well. Pointer ties are terminal.
        self.places().iter().any(|place| killed.covers(place) || place.covers(killed))
    }

    /// Where the result may point; empty when that is nowhere or unknown.
    pub fn places(&self) -> &[Place] {
        match self {
            Self::Explicit(places) => places,
            Self::Elided(place) => std::slice::from_ref(place),
            Self::None | Self::Unknown => &[],
        }
    }
}

/// A signature read at one call. No body of another function is consulted: effects are
/// a pure function of the signature and argument origins (spec §17.1, invariant 3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallEffects {
    pub kills: Vec<Kill>,
    pub result: ReturnTie,
    /// `e: &mut Enemy in world`, in caller coordinates, one entry per parameter. These
    /// are contracts for the body/call checker, not a reason to overwrite an argument's
    /// independently known origin.
    pub parameter_ties: Vec<ParameterTie>,
    /// Some kill's origin is unknown. Consumers must not treat the known kills as a
    /// proof that this call kills nothing else, or use them to narrow its caller's clause.
    pub incomplete: bool,
}

/// Translate every tie in a signature to the caller. The checker has already validated
/// the signature, but failures are returned intact for incremental/editor callers.
pub fn call_effects(scope: &Scope<'_>, bindings: &Bindings) -> Result<CallEffects, ties::Error> {
    let mut effects = CallEffects {
        kills: Vec::new(),
        result: ReturnTie::None,
        parameter_ties: Vec::new(),
        incomplete: false,
    };
    for path in &scope.sig.kills {
        match normalize(scope, path)?.bind(bindings) {
            Some(place) => effects.kills.push(Kill { place, clause: Some(path.span) }),
            None => effects.incomplete = true,
        }
    }
    if scope.sig.kills_unknown {
        // An imported C++ receiver is an implicit reference argument too.
        if scope.sig.takes_self == Some(true) {
            effects.unknown_kill(bindings.receiver.as_ref());
        }
        for (i, param) in scope.sig.params.iter().enumerate() {
            if mutable_input(&param.ty) {
                effects.unknown_kill(bindings.params.get(i).and_then(Option::as_ref));
            }
        }
    }
    for param in &scope.sig.params {
        let tied = if let Some(path) = &param.tie {
            match normalize(scope, path)?.bind(bindings) {
                Some(place) => ParameterTie::Known(place),
                None => ParameterTie::Unknown,
            }
        } else {
            ParameterTie::Unspecified
        };
        effects.parameter_ties.push(tied);
    }
    effects.result = if !scope.sig.ret_ties.is_empty() {
        let mut places = Vec::new();
        for path in &scope.sig.ret_ties {
            if let Some(place) = normalize(scope, path)?.bind(bindings) {
                places.push(place);
            }
        }
        if places.len() == scope.sig.ret_ties.len() {
            ReturnTie::Explicit(places)
        } else {
            ReturnTie::Unknown
        }
    } else if dz_check::ties::holds_view(scope.table, &scope.sig.ret, false) {
        // With no `in`, a result holding a view is in its one view input, whole: a
        // reference, or a struct, tuple or optional holding one (`iter()`'s `SliceIter`).
        // The checker rejects none or several (`DZ0570`, `DZ0571`), and what it leaves
        // alone, a raw pointer from nothing, stays unknown here. A signature read at a
        // call has its type parameters substituted, so a `T?` that became a `str?` lands
        // here too; `Signature::effects` takes that back, since `T` was the element.
        let mut inputs = Vec::new();
        if scope.sig.takes_self.is_some() {
            inputs.push(bindings.receiver.as_ref());
        }
        for (i, param) in scope.sig.params.iter().enumerate() {
            if points(&param.ty) {
                inputs.push(bindings.params.get(i).and_then(Option::as_ref));
            }
        }
        match inputs.as_slice() {
            [Some(place)] => ReturnTie::Elided((*place).clone()),
            _ => ReturnTie::Unknown,
        }
    } else if contains_views(&scope.sig.ret, scope.table) {
        // A view only through a type parameter: `T` may be one, and where it points is
        // wherever the value came from, which no signature says.
        ReturnTie::Unknown
    } else {
        ReturnTie::None
    };
    Ok(effects)
}

impl CallEffects {
    fn unknown_kill(&mut self, place: Option<&Place>) {
        match place {
            Some(place) => self.kills.push(Kill { place: place.clone(), clause: None }),
            None => self.incomplete = true,
        }
    }
}

fn mutable_input(ty: &Ty) -> bool {
    matches!(ty, Ty::Ref { mutable: true, .. } | Ty::Ptr { mutable: true, .. })
}

fn points(ty: &Ty) -> bool {
    matches!(ty, Ty::Ref { .. } | Ty::Ptr { .. } | Ty::Slice { .. } | Ty::Str | Ty::CStr)
}

/// Whether a value of `ty` may hold a view somewhere inside it, other than through a
/// pointer field, which stays unchecked (spec §6.7). A type parameter may.
pub(crate) fn contains_views(ty: &Ty, table: &TypeTable) -> bool {
    dz_check::ties::holds_view(table, ty, true)
}

/// The narrowest set of places describing a collection of effects, in first-occurrence
/// order. Repeated effects and descendants already covered by a whole-place kill add
/// nothing. This does not merge siblings: killing both fields need not kill inline
/// storage belonging to their enclosing value.
pub fn minimal(places: &[Place]) -> Vec<Place> {
    let mut out: Vec<Place> = Vec::new();
    for place in places {
        if out.iter().any(|p| p.covers(place)) {
            continue;
        }
        out.retain(|p| !place.covers(p));
        out.push(place.clone());
    }
    out
}

/// Clause disagreements, with indices into the supplied declarations/effects so the
/// body walker can retain the original spans and offer one complete replacement clause.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Mismatch {
    Missing { actual: usize },
    Unused { declared: usize },
    TooBroad { declared: usize, actual: Vec<usize> },
}

/// Compare an exact body summary with its clause (spec §6.7). Call only once the body
/// analysis is complete; unknown provenance must not become a spurious "unused" clause.
/// Trait requirements, extern declarations and function types use an upper bound instead
/// and must not be checked for unused/broad declarations.
pub fn compare(declared: &[Place], actual: &[Place]) -> Vec<Mismatch> {
    let one = |places: &[Place]| places.iter().map(|p| vec![p.clone()]).collect::<Vec<_>>();
    compare_ties(&one(declared), &one(actual))
}

/// [`compare`], over the ties each path names rather than the path as written ([`ties`]):
/// `s@text` and `s@text@items` are one clause when a `List<u8>` owns nothing else.
pub fn compare_ties(declared: &[Vec<Place>], actual: &[Vec<Place>]) -> Vec<Mismatch> {
    let mut out = Vec::new();
    for (i, leaves) in actual.iter().enumerate() {
        if leaves.iter().any(|leaf| !declared.iter().flatten().any(|d| d.covers(leaf))) {
            out.push(Mismatch::Missing { actual: i });
        }
    }
    for (i, declared) in declared.iter().enumerate() {
        // A broader actual kill already produces Missing at the operation. The narrow
        // declaration does describe memory that dies; reporting Unused as well cascades.
        if !declared.is_empty()
            && declared.iter().all(|d| actual.iter().flatten().any(|a| a.covers(d)))
        {
            continue;
        }
        let under: Vec<usize> = actual
            .iter()
            .enumerate()
            .filter_map(|(i, a)| {
                a.iter().any(|a| declared.iter().any(|d| d.covers(a))).then_some(i)
            })
            .collect();
        if under.is_empty() {
            out.push(Mismatch::Unused { declared: i });
        } else {
            out.push(Mismatch::TooBroad { declared: i, actual: under });
        }
    }
    out
}

/// The ties `place` names, of type `root` at its local (spec §6.7: a path that ends
/// without a tie means every tie under it). Each is the memory behind a tied pointer, a
/// union's or optional's payload, the elements of a type with `[*]`, or what an unknown
/// type might own; a plain field names none. Clauses are compared over these.
pub fn ties(scope: &Scope<'_>, place: &Place, root: &Ty) -> Vec<Place> {
    // A reference or pointer parameter's place is what it points at.
    let mut ty = Some(root.peeled().clone());
    for (i, step) in place.steps.iter().enumerate() {
        ty = match (ty, step) {
            (Some(t), Step::Field(name)) => match field(scope, &t, name) {
                Some(next) => Some(next),
                // A field its type does not have, reached through a cast (a `^mut void`
                // read as a struct): no path can name it, so what holds it is the tie,
                // named whole, as a clause would write it.
                None => {
                    let held = Place { local: place.local, steps: place.steps[..i].to_vec() };
                    return vec![canonical_place(scope.table, scope.res, root, &held)];
                }
            },
            (Some(t), Step::Elements) => ties::elements(scope, &t).flatten(),
            _ => None,
        };
    }
    let mut out = Vec::new();
    match ty {
        Some(ty) => collect_ties(scope, place, &ty, &mut out, 0),
        // Memory named whole: the storage behind a pointer, or an iterable's entries.
        None => out.push(place.clone()),
    }
    // A field read through another type's tie is that tie: a `SlotMap`'s three lists are
    // its one `@items`.
    out.iter().map(|p| canonical_place(scope.table, scope.res, root, p)).collect()
}

fn collect_ties(scope: &Scope<'_>, place: &Place, ty: &Ty, out: &mut Vec<Place>, depth: usize) {
    if depth > 6 {
        out.push(place.clone());
        return;
    }
    match ty {
        // What a `^mut void` points at may own anything, so it is named whole.
        Ty::Optional(_) | Ty::Param(_) | Ty::Assoc(_) | Ty::Void => out.push(place.clone()),
        Ty::Array { elem, .. } => {
            collect_ties(scope, &place.followed_by(&[Step::Elements]), elem, out, depth + 1)
        }
        Ty::Tuple(_) if body::moves_memory(ty, scope.table) => out.push(place.clone()),
        Ty::Named { sym, args } => match scope.table.get(*sym) {
            Some(dz_check::ItemSig::Union { .. }) => {
                if body::moves_memory(ty, scope.table) {
                    out.push(place.clone());
                }
            }
            Some(dz_check::ItemSig::Struct(s)) => {
                let types = scope.table.field_types(*sym, args, &s.fields);
                // A `drop` freeing memory behind an untied `own` pointer owns something no
                // path inside it names, so the value is named whole.
                if s.drop.is_some()
                    && s.fields
                        .iter()
                        .zip(&types)
                        .any(|(f, t)| f.tie.is_none() && matches!(t, Ty::OwnPtr { .. }))
                {
                    out.push(place.clone());
                    return;
                }
                let mut seen = Vec::new();
                for (f, t) in s.fields.iter().zip(types) {
                    let name = canonical_field(scope.table, *sym, &f.name);
                    if seen.contains(&name) {
                        continue;
                    }
                    seen.push(name.clone());
                    let at = place.followed_by(&[Step::Field(name)]);
                    match t {
                        Ty::Ptr { .. } | Ty::OwnPtr { .. } if f.tie.is_some() => {
                            out.push(at.followed_by(&[Step::Storage]))
                        }
                        // An untied pointer is unchecked (§6.7).
                        Ty::Ptr { .. } | Ty::OwnPtr { .. } => {}
                        t => collect_ties(scope, &at, &t, out, depth + 1),
                    }
                }
                // Behind a pointer, the elements are not among the fields, so `[*]` is
                // reached through what `Index` or `Iterable` says (§6.7).
                match ties::elements(scope, ty) {
                    Some(Some(e)) => collect_ties(
                        scope,
                        &place.followed_by(&[Step::Elements]),
                        &e,
                        out,
                        depth + 1,
                    ),
                    // Entries that are values computed on the way, as a `String`'s
                    // characters are, are no memory of its own to name.
                    Some(None) => {
                        let item = ties::iterated(scope, ty);
                        if item.map_or(true, |t| {
                            points(&t)
                                || contains_views(&t, scope.table)
                                || body::moves_memory(&t, scope.table)
                        }) {
                            out.push(place.followed_by(&[Step::Elements]))
                        }
                    }
                    None => {}
                }
            }
            _ => {}
        },
        _ => {}
    }
}
