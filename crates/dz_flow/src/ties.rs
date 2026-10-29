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
pub use body::{
    analyse_body, BodyAnalysis, BodyKill, Incomplete, InvalidReturn, InvalidUse, KillKind,
};

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

/// Normalize a checked tie path. `in @name` on a non-pointer is a public alias of a
/// field; on a pointer it names the storage behind that field (spec §6.7).
pub fn normalize(scope: &Scope<'_>, path: &dz_ast::TiePath) -> Result<Path, ties::Error> {
    let resolved = ties::resolve(scope, path)?;
    let mut ty = match resolved.root {
        Root::SelfValue => scope.self_ty.clone().unwrap_or(Ty::Error),
        Root::Param(i) => scope.sig.params[i].ty.clone(),
    };
    let mut steps = Vec::new();
    for step in resolved.steps {
        match step {
            ties::Step::Field(name) => {
                let canonical = match ty.peeled() {
                    Ty::Named { sym, .. } => canonical_field(scope.table, *sym, &name),
                    _ => name.clone(),
                };
                ty = field(scope, &ty, &name).ok_or_else(|| internal_path_error(path.span))?;
                steps.push(Step::Field(canonical));
            }
            ties::Step::Tie(name) => {
                let Ty::Named { sym, args } = ty.peeled() else {
                    return Err(internal_path_error(path.span));
                };
                let Some(f) = scope
                    .table
                    .struct_sig(*sym)
                    .and_then(|s| s.fields.iter().find(|f| f.tie.as_deref() == Some(&name)))
                else {
                    return Err(internal_path_error(path.span));
                };
                let next = scope.table.field_types(*sym, args, std::slice::from_ref(f)).remove(0);
                steps.push(Step::Field(f.name.clone()));
                if matches!(next, Ty::Ptr { .. } | Ty::OwnPtr { .. }) {
                    steps.push(Step::Storage);
                }
                ty = next;
            }
            ties::Step::Elements => {
                // An Iterable-only path ends here; there is no element type to follow.
                ty = ties::elements(scope, &ty).flatten().unwrap_or(Ty::Error);
                steps.push(Step::Elements);
            }
        }
    }
    Ok(Path { root: resolved.root, steps })
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
    let Ty::Named { sym, args } = ty.peeled() else { return None };
    let f = scope.table.struct_sig(*sym)?.fields.iter().find(|f| f.name == name)?;
    Some(scope.table.field_types(*sym, args, std::slice::from_ref(f)).remove(0))
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
    Explicit(Place),
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
        match self {
            // An `in` path ending at a value means all ties beneath that value, so a
            // kill inside it may end this view as well. Pointer ties are terminal.
            Self::Explicit(place) | Self::Elided(place) => {
                killed.covers(place) || place.covers(killed)
            }
            Self::None | Self::Unknown => false,
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
    effects.result = if let Some(path) = &scope.sig.ret_tie {
        match normalize(scope, path)?.bind(bindings) {
            Some(place) => ReturnTie::Explicit(place),
            None => ReturnTie::Unknown,
        }
    } else if points(&scope.sig.ret) {
        let mut inputs = Vec::new();
        if scope.sig.takes_self.is_some() {
            inputs.push(bindings.receiver.as_ref());
        }
        // §6.7 elides only with one reference input. Whether a pointer, slice or `str`
        // input counts is not settled (T-0107), so one beside another input leaves the
        // result unknown rather than choosing between them.
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
        // A struct, tuple or optional holding a view (`SliceIter`'s slice) has no
        // elision rule yet, so where it points is unknown rather than nowhere.
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

/// Whether a value of `ty` holds a view somewhere inside it, other than through a
/// pointer field, which stays unchecked (spec §6.7).
pub(crate) fn contains_views(ty: &Ty, table: &TypeTable) -> bool {
    fn contains(ty: &Ty, table: &TypeTable, depth: usize) -> bool {
        if depth > 8 {
            return true;
        }
        match ty {
            Ty::Tuple(parts) => parts.iter().any(|p| contains(p, table, depth + 1)),
            Ty::Optional(inner) | Ty::Array { elem: inner, .. } => {
                contains(inner, table, depth + 1)
            }
            Ty::Named { sym, args } => table.struct_sig(*sym).is_some_and(|s| {
                table.field_types(*sym, args, &s.fields).iter().any(|f| {
                    // Pointer fields are the explicit unchecked boundary in §6.7;
                    // str/slice fields still make an aggregate a view.
                    !matches!(f, Ty::Ptr { .. } | Ty::OwnPtr { .. })
                        && contains(f, table, depth + 1)
                })
            }),
            Ty::Param(_) => true,
            _ => points(ty),
        }
    }
    contains(ty, table, 0)
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
    let mut out = Vec::new();
    for (i, place) in actual.iter().enumerate() {
        if !declared.iter().any(|d| d.covers(place)) {
            out.push(Mismatch::Missing { actual: i });
        }
    }
    for (i, declared) in declared.iter().enumerate() {
        // A broader actual kill already produces Missing at the operation. The narrow
        // declaration does describe memory that dies; reporting Unused as well cascades.
        if actual.iter().any(|a| a.covers(declared)) {
            continue;
        }
        let under: Vec<usize> = actual
            .iter()
            .enumerate()
            .filter_map(|(i, a)| declared.covers(a).then_some(i))
            .collect();
        if under.is_empty() {
            out.push(Mismatch::Unused { declared: i });
        } else {
            out.push(Mismatch::TooBroad { declared: i, actual: under });
        }
    }
    out
}
