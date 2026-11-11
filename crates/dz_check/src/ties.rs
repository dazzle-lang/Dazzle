//! Tie paths, resolved (spec §6.7).
//!
//! A signature keeps its `kills` clause and its `in` ties as written, since it is collected
//! before every type's fields are, and a struct keeps its `tie` declarations the same way.
//! [`resolve`] reads a signature's path once they are: which parameter it starts from, and
//! the field, element or tie each step names; [`export_path`] does the same for a path in
//! a `tie` declaration, which starts at one of the type's own fields. The checker reports
//! what [`check`] and [`check_struct`] find wrong, once each; `dz_flow` calls [`resolve`]
//! and [`export_path`] for the paths themselves, which by then all resolve.

use dz_ast::{TiePath, TieStep, Vis};
use dz_diag::codes;
use dz_hir::ty::{FnKills, IntTy, Ty};
use dz_project::ModuleId;
use dz_resolve::{Resolution, SymbolId};
use dz_span::Span;

use crate::{FnSig, ParamSig, TieSig, TraitRef, TypeTable};

/// A tie path with every name looked up.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Tie {
    pub root: Root,
    pub steps: Vec<Step>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Root {
    SelfValue,
    /// The index of a parameter, not counting `self`.
    Param(usize),
    /// A static: one place for the whole program, which every caller can name too (spec
    /// §6.7, D-0094 point 7).
    Static(SymbolId),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Step {
    Field(String),
    /// `[*]`
    Elements,
    /// `@items`
    Tie(String),
    /// `@home`: the region or heap the value's memory is in, which no struct declares
    /// (D-0097). Only ever the last step.
    Home,
}

/// What a function's tie paths are resolved against.
pub struct Scope<'t> {
    pub table: &'t TypeTable,
    pub res: &'t Resolution,
    /// The module the function is written in, for `pub(module)` fields.
    pub module: ModuleId,
    pub name: &'t str,
    pub sig: &'t FnSig,
    /// `self`'s type, in a method.
    pub self_ty: Option<Ty>,
    /// The type whose private members the function sees: the one it is a method of.
    pub privileged: Option<SymbolId>,
}

/// A path that does not resolve, or a clause that cannot be what it says.
pub struct Error {
    pub code: &'static str,
    pub span: Span,
    pub message: String,
    pub label: String,
    pub help: String,
}

fn error(
    code: &'static str,
    span: Span,
    message: String,
    label: impl Into<String>,
    help: impl Into<String>,
) -> Error {
    Error { code, span, message, label: label.into(), help: help.into() }
}

/// How far ties may stand for other types' ties before the chain is taken to be a cycle:
/// `tie @a = m_list[*]@a` in a type its own list holds.
const MAX_DEPTH: usize = 16;

/// Who names a path, which decides what it may see (spec §4).
struct Walker<'t> {
    table: &'t TypeTable,
    res: &'t Resolution,
    module: ModuleId,
    /// The type whose private members are visible.
    privileged: Option<SymbolId>,
    /// How visible what the path names must be: the function's own visibility, since a
    /// clause is part of its signature. `None` in a type's own `tie`, whose paths name
    /// private storage by design.
    callers: Option<Vis>,
    /// The function or type naming the path, for messages.
    name: &'t str,
}

impl<'t> Walker<'t> {
    fn of(scope: &Scope<'t>) -> Self {
        // A free function's own privacy is the module's, since anything in the module may
        // call it (spec §4).
        let callers = match (scope.privileged, scope.sig.vis) {
            (None, Vis::Private) => Vis::Module,
            (_, v) => v,
        };
        Walker {
            table: scope.table,
            res: scope.res,
            module: scope.module,
            privileged: scope.privileged,
            callers: Some(callers),
            name: scope.name,
        }
    }

    /// Inside `sym`'s own declaration, where its private fields are what a tie names.
    fn inside(table: &'t TypeTable, res: &'t Resolution, sym: SymbolId) -> Self {
        let s = res.symbol(sym);
        Walker { table, res, module: s.module, privileged: Some(sym), callers: None, name: &s.name }
    }
}

/// Every mistake in `scope`'s signature: a path that names nothing or something its
/// callers cannot see, a `kills` through something the function cannot change, and an
/// `in` on something that points into nothing. Reported once each.
pub fn check(scope: &Scope<'_>, params: &[dz_ast::Param]) -> Vec<Error> {
    let mut out = Vec::new();
    for path in &scope.sig.kills {
        match resolve(scope, path) {
            Ok(tie) if tie.steps.contains(&Step::Home) => out.push(home_misplaced(
                path,
                "a home is never killed, since it is the region or heap the value was placed in",
            )),
            Ok(tie) => {
                if let Err(e) = changeable(scope, &tie, path) {
                    out.push(e);
                }
            }
            Err(e) => out.push(e),
        }
    }
    for path in &scope.sig.ret_ties {
        match resolve(scope, path) {
            Ok(tie) if tie.steps.contains(&Step::Home) => out.push(home_misplaced(
                path,
                "`@home` ends a path: it names where the value's memory is, and nothing inside it",
            )),
            Ok(_) if !holds_view(scope.table, &scope.sig.ret, true) => {
                let mut e = points_nowhere(scope, "the result", &scope.sig.ret, path.span);
                if owns_memory(scope.table, &scope.sig.ret) {
                    // An owned result is tied to where its memory is, not to where it points.
                    e.help = format!(
                        "an owned result taken out of an input says it lives in that input's \
                         home: `in {}@home` (D-0097)",
                        path.root_spelling()
                    );
                }
                out.push(e)
            }
            Ok(_) => {}
            Err(e) => out.push(e),
        }
    }
    for path in &scope.sig.ret_home {
        match resolve(scope, path) {
            // A type parameter may be given an owned value, so only a result that is a view
            // whatever it is given is refused.
            Ok(_) if holds_view(scope.table, &scope.sig.ret, false) => out.push(home_misplaced(
                path,
                "a view says where it points with `in` and a path without `@home`; `@home` is \
                 for an owned result",
            )),
            Ok(_) if !owns_memory(scope.table, &scope.sig.ret) => {
                let n = crate::type_name(scope.res, &scope.sig.ret);
                out.push(home_misplaced(
                    path,
                    &format!("the result is `{n}`, which owns no memory, so it has no home"),
                ))
            }
            Ok(_) => {}
            Err(e) => out.push(e),
        }
    }
    for send in &scope.sig.sends {
        for (path, giving) in [(&send.what, true), (&send.to, false)] {
            match resolve(scope, path) {
                Ok(tie) if tie.steps.contains(&Step::Home) => out.push(home_misplaced(
                    path,
                    "a `sends` names values, which take their homes with them",
                )),
                Ok(tie) => {
                    if let Err(e) = sendable(scope, &tie, path, giving) {
                        out.push(e);
                    }
                }
                Err(e) => out.push(e),
            }
        }
    }
    for (i, (p, sig)) in params.iter().zip(&scope.sig.params).enumerate() {
        let Some(path) = &p.tie else { continue };
        match resolve(scope, path) {
            Ok(tie) if tie.steps.contains(&Step::Home) => out.push(home_misplaced(
                path,
                "a reference parameter says where it points, which is never a home",
            )),
            Ok(_) if !points(&sig.ty) => {
                out.push(points_nowhere(scope, &format!("`{}`", p.name.name), &sig.ty, path.span))
            }
            Ok(tie) if tie.root == Root::Param(i) => out.push(error(
                codes::TIE_ON_NON_REFERENCE,
                path.root.span,
                format!("`{}` cannot point into itself", p.name.name),
                "the parameter being declared",
                "`in` names another parameter, or `self`, that this reference may point into \
                 (spec §6.7)",
            )),
            Ok(_) => {}
            Err(e) => out.push(e),
        }
    }
    out
}

/// Every mistake in `sym`'s own `tie` declarations: a tie declared twice, `[*]` on a type
/// with no elements, and a path that names none of the type's memory.
pub fn check_struct(table: &TypeTable, res: &Resolution, sym: SymbolId) -> Vec<Error> {
    let mut out = Vec::new();
    let Some(s) = table.struct_sig(sym) else { return out };
    let args: Vec<Ty> = table.type_params(sym).iter().map(|p| Ty::Param(p.param.clone())).collect();
    let self_ty = Ty::Named { sym, args: args.clone() };
    let type_name = &res.symbol(sym).name;
    for (i, t) in s.ties.iter().enumerate() {
        let shown = t.name.as_ref().map_or("[*]".to_owned(), |n| format!("@{n}"));
        if t.name.as_deref() == Some("home") {
            out.push(error(
                codes::HOME_TIE_RESERVED,
                t.span,
                format!("`{type_name}` declares `@home`, which every value that owns memory has"),
                "reserved",
                "`@home` is the region or heap a value was placed in, which no struct declares; \
                 give this tie another name (D-0097)",
            ));
            continue;
        }
        if s.ties[..i].iter().any(|e| e.name == t.name) {
            out.push(error(
                codes::TIE_DECLARED_TWICE,
                t.span,
                format!("`{type_name}` declares `{shown}` twice"),
                "declared again here",
                format!(
                    "list everything `{shown}` stands for in one declaration, `tie {shown} = \
                     a, b` (spec §6.7)"
                ),
            ));
            continue;
        }
        if t.name.is_none() && elements_in(table, &self_ty).is_none() {
            out.push(error(
                codes::TIE_PATH_UNRESOLVED,
                t.span,
                format!("`{type_name}` has no elements"),
                "nothing for `[*]` to name",
                "`tie [*]` says where the elements of a type with an `Index` or `Iterable` \
                 implementation live (spec §6.7)",
            ));
        }
        for path in &t.paths {
            if let Err(e) = export_path(table, res, sym, &args, path) {
                out.push(e);
            }
        }
    }
    out
}

/// Whether a `T` points into memory something else owns: a reference, a pointer, a slice
/// or a `str`.
fn points(t: &Ty) -> bool {
    matches!(t, Ty::Ref { .. } | Ty::Ptr { .. } | Ty::Slice { .. } | Ty::Str | Ty::CStr)
}

/// Whether a value of `t` holds a view: it points itself, or it is a struct, tuple,
/// optional or array with one inside, other than behind a pointer field, which stays
/// unchecked (spec §6.7). `generic` is what a type parameter counts as, since it may be
/// given a view or not.
pub fn holds_view(table: &TypeTable, t: &Ty, generic: bool) -> bool {
    holds(table, t, generic, points, true, 0)
}

/// Whether an input of type `t` is one a result may be in: a reference, a pointer, a slice
/// or a `str`, or a view taken by value, a view struct or a `view` parameter's value, which
/// stands for what it views (spec §6.7, D-0093).
pub fn view_input(table: &TypeTable, t: &Ty) -> bool {
    points(t) || table.placed_as_view(t)
}

/// Whether a value of `t` holds a view whose `leaf` is true, as [`holds_view`] walks it.
/// `view_params` counts a `view` parameter and a projection of a `view type` as views,
/// since either may be given one.
fn holds(
    table: &TypeTable,
    t: &Ty,
    generic: bool,
    leaf: fn(&Ty) -> bool,
    view_params: bool,
    depth: usize,
) -> bool {
    if depth > MAX_DEPTH {
        return true;
    }
    let inner = |t: &Ty| holds(table, t, generic, leaf, view_params, depth + 1);
    match t {
        Ty::Tuple(parts) => parts.iter().any(inner),
        Ty::Optional(t) | Ty::Array { elem: t, .. } => inner(t),
        Ty::Named { sym, args } => table.struct_sig(*sym).is_some_and(|s| {
            table
                .field_types(*sym, args, &s.fields)
                .iter()
                .any(|f| !matches!(f, Ty::Ptr { .. } | Ty::OwnPtr { .. }) && inner(f))
        }),
        Ty::Param(p) => generic || (view_params && p.view),
        Ty::Assoc(_) => view_params && table.placed_as_view(t),
        _ => leaf(t),
    }
}

/// A result that holds a view and says nothing of where: wrong unless there is exactly one
/// view input for it to be in (spec §6.7).
pub struct UntiedResult {
    pub error: Error,
    /// ` in (a, b)`, to insert after the result's type: every input, which is never wrong,
    /// for the author to narrow. `None` where the source cannot take it.
    pub fix: Option<(Span, String)>,
}

/// Every reference a function returns is tied to something its caller can see die: with no
/// `in`, the result is in its one view input, so none or several is an error. A result that
/// holds a view only through a plain type parameter needs no tie, since `T` is never a view
/// (D-0093, point 8); a raw pointer or `cstr` with no input, which may own what it points at, as one made in
/// `unsafe` does, a result that may come out of a view a parameter holds by value, and an
/// `extern fn` with no input, whose result is C's memory, outside what the checker sees.
pub fn untied_result(scope: &Scope<'_>, f: &dz_ast::FnDecl) -> Option<UntiedResult> {
    let ret = &scope.sig.ret;
    if !scope.sig.ret_ties.is_empty() || !holds_view(scope.table, ret, false) {
        return None;
    }
    let ret_span = f.ret.as_ref().map_or(f.name.span, |r| r.span);
    // Every token a macro wrote is at its invocation (§12), so the result's type does not
    // follow the function's name, and a fix belongs in the macro.
    let fixable = ret_span.end > f.name.span.end;
    let mut inputs = Vec::new();
    if scope.sig.takes_self.is_some() {
        inputs.push("self".to_owned());
    }
    inputs.extend(
        scope.sig.params.iter().filter(|p| view_input(scope.table, &p.ty)).map(|p| p.name.clone()),
    );
    let shown = crate::type_name(scope.res, ret);
    let name = scope.name;
    match inputs.len() {
        1 => None,
        0 => {
            // Only a reference, slice or `str` must be tied; raw pointers and `cstr`s, on
            // their own or in an aggregate, may not be views at all, and neither may what
            // a `view` parameter is given.
            let tied = |t: &Ty| matches!(t, Ty::Ref { .. } | Ty::Slice { .. } | Ty::Str | Ty::CStr);
            // `^str` and `^[]T` are pointers, which may be untied as any pointer may.
            let pointer =
                matches!(ret, Ty::Optional(i) if matches!(**i, Ty::Str | Ty::Slice { .. }));
            if pointer
                || !holds(scope.table, ret, false, tied, false, 0)
                || scope.sig.params.iter().any(|p| holds_view(scope.table, &p.ty, false))
            {
                return None;
            }
            // Text from nowhere is usually a literal's, which is what `const &str` says;
            // but the body may build it some other way, so it is only offered.
            let fix = (fixable && *ret == Ty::Str).then(|| (ret_span, "const &str".to_owned()));
            Some(UntiedResult {
                error: error(
                    codes::RESULT_TIED_TO_NOTHING,
                    ret_span,
                    format!(
                        "`{name}` returns `{shown}`, which holds a view, but takes nothing it \
                         could point into"
                    ),
                    "points into nothing its caller can see",
                    "return text that never dies as `const &str`, return something owned \
                     (`String`, `List`), take what the result points into as a parameter, or \
                     name the static it points into, `in SCORES.list@items` (spec §6.7)",
                ),
                fix,
            })
        }
        _ => {
            let listed = inputs.iter().map(|i| format!("`{i}`")).collect::<Vec<_>>().join(" or ");
            let all = format!("in ({})", inputs.join(", "));
            let fix = fixable.then(|| (Span::at(ret_span.end as usize), format!(" {all}")));
            Some(UntiedResult {
                error: error(
                    codes::RESULT_TIE_AMBIGUOUS,
                    ret_span,
                    format!(
                        "`{name}` returns `{shown}`, which may point into {listed}: the \
                         signature must say which"
                    ),
                    "no `in` says where this points",
                    format!(
                        "say what the result points into after its type: `in {}`, or `{all}` \
                         if it may be in any of them (spec §6.7)",
                        inputs[0]
                    ),
                ),
                fix,
            })
        }
    }
}

/// [`untied_result`] for a function type, written at `span`: a result holding a view, with
/// no `in`, is in the type's one view input, so none or several is an error, with the same
/// exemptions. There is no fix to offer, since the type's parameters may have no names.
fn untied_fn_type(scope: &Scope<'_>, names: &[String], span: Span) -> Option<Error> {
    let sig = scope.sig;
    if !sig.ret_ties.is_empty() || !holds_view(scope.table, &sig.ret, false) {
        return None;
    }
    let inputs: Vec<String> = sig
        .params
        .iter()
        .enumerate()
        .filter(|(_, p)| view_input(scope.table, &p.ty))
        .map(|(i, _)| match names.get(i).filter(|n| !n.is_empty()) {
            Some(n) => format!("`{n}`"),
            None => format!("its parameter {}", i + 1),
        })
        .collect();
    let shown = crate::type_name(scope.res, &sig.ret);
    match inputs.len() {
        1 => None,
        0 => {
            let tied = |t: &Ty| matches!(t, Ty::Ref { .. } | Ty::Slice { .. } | Ty::Str | Ty::CStr);
            if !holds(scope.table, &sig.ret, false, tied, false, 0)
                || sig.params.iter().any(|p| holds_view(scope.table, &p.ty, true))
            {
                return None;
            }
            Some(error(
                codes::RESULT_TIED_TO_NOTHING,
                span,
                format!(
                    "this function type returns `{shown}`, which holds a view, but takes nothing \
                     it could point into"
                ),
                "points into nothing its caller can see",
                "a function stored here could only return memory that never dies: make the \
                 result `const &str`, something owned (`String`, `List`), or take what it \
                 points into as a parameter (spec §6.7)",
            ))
        }
        _ => {
            let named = names.len() == sig.params.len() && names.iter().all(|n| !n.is_empty());
            let help = if named {
                let all = sig
                    .params
                    .iter()
                    .filter(|p| view_input(scope.table, &p.ty))
                    .map(|p| p.name.as_str())
                    .collect::<Vec<_>>();
                format!(
                    "say what the result points into after its type: `in {}`, or `in ({})` if \
                     it may be in any of them (spec §6.7)",
                    all[0],
                    all.join(", ")
                )
            } else {
                "name the parameters and say which the result points into: `fn(a: &T, b: &T) \
                 -> &T in a`, or `in (a, b)` if it may be in either (spec §6.7)"
                    .to_owned()
            };
            Some(error(
                codes::RESULT_TIE_AMBIGUOUS,
                span,
                format!(
                    "this function type returns `{shown}`, which may point into {}: the type \
                     must say which",
                    inputs.join(" or ")
                ),
                "no `in` says where this points",
                help,
            ))
        }
    }
}

fn points_nowhere(scope: &Scope<'_>, what: &str, t: &Ty, span: Span) -> Error {
    let n = crate::type_name(scope.res, t);
    error(
        codes::TIE_ON_NON_REFERENCE,
        span,
        format!("{what} has type `{n}`, which points into nothing"),
        "nothing to say this of",
        "`in` says where a reference, a pointer, a slice or a `str` points (spec §6.7)",
    )
}

/// `@home` at the end of `path` (D-0097): whether a parsed path is one, so a result's `in`
/// can be told apart as saying where an owned value lives.
pub fn ends_at_home(path: &TiePath) -> bool {
    matches!(path.steps.last(), Some(TieStep::Tie(t)) if t.name == "home")
}

/// `@home` in `path` where it means nothing, and why.
fn home_misplaced(path: &TiePath, why: &str) -> Error {
    let span = path
        .steps
        .iter()
        .find_map(|s| match s {
            TieStep::Tie(t) if t.name == "home" => Some(t.span),
            _ => None,
        })
        .unwrap_or(path.span);
    error(
        codes::HOME_MISPLACED,
        span,
        format!("`@home` cannot be named here: {why}"),
        "`@home` here",
        "`@home` ends a path after a result's `in`, for an owned result taken out of an input: \
         `-> T in xs@home` (D-0097)",
    )
}

/// Whether a value of `t`, or what `t` refers to, may own memory from an allocator, and so
/// have a home (D-0097). A type parameter may; a reference or pointer is asked of what it
/// points at, and an optional of what it holds.
pub fn owns_memory(table: &TypeTable, t: &Ty) -> bool {
    match t.peeled() {
        Ty::Optional(inner) | Ty::Fallible(inner) => owns_memory(table, inner),
        // A trait's `self` in its declaration, which an implementation may give memory.
        Ty::Named { sym, .. } if matches!(table.get(*sym), Some(crate::ItemSig::Trait(_))) => true,
        other => table.needs_drop(other),
    }
}

/// A `sends` path (D-0097). What is sent, `giving`, comes from something the function owns
/// (a by-value parameter) or can change (a `&mut` parameter, `&mut self`, a static); what
/// it is sent to, the function must be able to change.
fn sendable(scope: &Scope<'_>, tie: &Tie, path: &TiePath, giving: bool) -> Result<(), Error> {
    let (what, ty) = match tie.root {
        Root::Static(_) => return Ok(()),
        Root::SelfValue => {
            if scope.sig.takes_self == Some(true) {
                return Ok(());
            }
            ("`self`".to_owned(), "taken as `&self`".to_owned())
        }
        Root::Param(i) => {
            let p = &scope.sig.params[i];
            let by_value = !matches!(p.ty, Ty::Ref { .. } | Ty::Ptr { .. } | Ty::Slice { .. });
            if (giving && by_value)
                || matches!(
                    p.ty,
                    Ty::Ref { mutable: true, .. }
                        | Ty::Ptr { mutable: true, .. }
                        | Ty::Slice { mutable: true, .. }
                )
            {
                return Ok(());
            }
            let n = crate::type_name(scope.res, &p.ty);
            (format!("`{}`", p.name), format!("taken as `{n}`"))
        }
    };
    let message = if giving {
        format!("`{}` sends from {what}, which it neither owns nor can change", scope.name)
    } else {
        format!("`{}` sends to {what}, which it cannot change", scope.name)
    };
    Err(error(
        codes::SENDS_NEEDS_MUT,
        path.root.span,
        message,
        ty,
        "a function sends what it was given by value, or takes through a `&mut` parameter or \
         `&mut self`, into a `&mut` parameter, `&mut self` or a static (D-0097)",
    ))
}

/// A `kills` path must start where the function can change something: a `&mut` or `^mut`
/// parameter, or `&mut self`. What it was given by value is its own, and its callers never
/// see that die.
fn changeable(scope: &Scope<'_>, tie: &Tie, path: &TiePath) -> Result<(), Error> {
    let (what, ty) = match tie.root {
        // Anything may change a static, which is why its clause says so (D-0094 point 7).
        Root::Static(_) => return Ok(()),
        Root::SelfValue => {
            if scope.sig.takes_self == Some(true) {
                return Ok(());
            }
            ("`self`".to_owned(), "taken as `&self`".to_owned())
        }
        Root::Param(i) => {
            let p = &scope.sig.params[i];
            if matches!(
                p.ty,
                Ty::Ref { mutable: true, .. }
                    | Ty::Ptr { mutable: true, .. }
                    | Ty::Slice { mutable: true, .. }
            ) {
                return Ok(());
            }
            let n = crate::type_name(scope.res, &p.ty);
            (format!("`{}`", p.name), format!("taken as `{n}`"))
        }
    };
    Err(error(
        codes::KILLS_NEEDS_MUT,
        path.root.span,
        format!("`{}` kills through {what}, which it cannot change", scope.name),
        ty,
        "a function kills only through a `&mut` or `^mut` parameter, or `&mut self`; what it \
         takes by value is its own, and dies with it (spec §6.7)",
    ))
}

/// The parameter and the fields, elements and ties `path` names (spec §6.7).
pub fn resolve(scope: &Scope<'_>, path: &TiePath) -> Result<Tie, Error> {
    let (root, ty) = if path.root.name == "self" {
        match (&scope.sig.takes_self, &scope.self_ty) {
            (Some(_), Some(t)) => (Root::SelfValue, t.clone()),
            _ => {
                return Err(error(
                    codes::TIE_PATH_UNRESOLVED,
                    path.root.span,
                    format!("`{}` has no `self`", scope.name),
                    "not a method",
                    "a tie path starts at one of the function's parameters, or at `self` in a \
                     method (spec §6.7)",
                ))
            }
        }
    } else {
        let param = match path.module.is_empty() {
            true => scope.sig.params.iter().position(|p| p.name == path.root.name),
            false => None,
        };
        match (param, static_root(scope, path)) {
            (Some(i), _) => (Root::Param(i), scope.sig.params[i].ty.clone()),
            (None, Some(sym)) => (Root::Static(sym), static_visible(scope, sym, path)?),
            (None, None) => {
                let written = path.root_spelling();
                return Err(error(
                    codes::TIE_PATH_UNRESOLVED,
                    path.root.span,
                    format!("`{written}` is not a parameter of `{}`, or a static", scope.name),
                    "no parameter or static of this name",
                    "a tie path starts at one of the function's parameters, at `self` in a \
                     method, or at a static (spec §6.7)",
                ));
            }
        }
    };
    let (steps, _, _) = Walker::of(scope).walk(ty, &path.steps, 0)?;
    Ok(Tie { root, steps })
}

/// The static `path`'s root names, as the signature's declaration resolved it.
fn static_root(scope: &Scope<'_>, path: &TiePath) -> Option<SymbolId> {
    let written: Vec<&str> =
        path.module.iter().map(|m| m.name.as_str()).chain([path.root.name.as_str()]).collect();
    scope
        .sig
        .statics
        .iter()
        .find(|(w, _)| w.iter().map(String::as_str).eq(written.iter().copied()))
        .map(|(_, sym)| *sym)
}

/// A static's type, once it is known that every caller of the function can name it: each
/// one names it in a clause of its own, up to `main` (spec §6.7).
fn static_visible(scope: &Scope<'_>, sym: SymbolId, path: &TiePath) -> Result<Ty, Error> {
    let s = scope.res.symbol(sym);
    let vis = if s.vis == Vis::Public { Vis::Public } else { Vis::Module };
    if !Walker::of(scope).sees_item(s.module, vis) {
        return Err(error(
            codes::TIE_PATH_NOT_VISIBLE,
            path.root.span,
            format!(
                "`{}` is less visible than `{}`, whose callers must see it",
                path.root_spelling(),
                scope.name
            ),
            "not visible to every caller",
            format!(
                "every caller of `{}` names what it kills in a clause of its own: make the \
                 static `pub`, or the function no more visible than the static (spec §6.7)",
                scope.name
            ),
        ));
    }
    Ok(match scope.table.get(sym) {
        Some(crate::ItemSig::Static(ty)) => ty.clone(),
        _ => Ty::Error,
    })
}

/// A path in one of `sym`'s `tie` declarations, at type arguments `args`: the steps from
/// the type itself, the root field first, and the type it ends at. The third part is why
/// nothing further can be named past it, when that is so; a path ending at a pointer field
/// names the memory behind it, so that is one such end (spec §6.7).
pub fn export_path(
    table: &TypeTable,
    res: &Resolution,
    sym: SymbolId,
    args: &[Ty],
    path: &TiePath,
) -> Result<(Vec<Step>, Ty, Option<String>), Error> {
    export_path_at(table, res, sym, args, path, 0)
}

fn export_path_at(
    table: &TypeTable,
    res: &Resolution,
    sym: SymbolId,
    args: &[Ty],
    path: &TiePath,
    depth: usize,
) -> Result<(Vec<Step>, Ty, Option<String>), Error> {
    let w = Walker::inside(table, res, sym);
    if depth > MAX_DEPTH {
        return Err(error(
            codes::TIE_PATH_UNRESOLVED,
            path.span,
            format!("the ties of `{}` stand for each other without end", w.name),
            "part of a cycle",
            "a tie names memory the type holds; name the fields themselves (spec §6.7)",
        ));
    }
    if path.root.name == "self" {
        return Err(error(
            codes::TIE_PATH_UNRESOLVED,
            path.root.span,
            "a tie stands for memory inside the type, so its path starts at a field".into(),
            "not a field",
            "name one of the type's fields: `tie @items = m_data` (spec §6.7)",
        ));
    }
    let mut steps = Vec::with_capacity(path.steps.len() + 1);
    steps.push(TieStep::Field(path.root.clone()));
    steps.extend(path.steps.iter().cloned());
    let (out, ty, mut ended) = w.walk(Ty::Named { sym, args: args.to_vec() }, &steps, depth)?;
    if out.contains(&Step::Home) {
        return Err(home_misplaced(
            path,
            "a tie stands for memory inside the type, which a home is not",
        ));
    }
    if ended.is_none() && matches!(ty, Ty::Ptr { .. } | Ty::OwnPtr { .. }) {
        ended = Some(format!("the memory `{}` points into", path.root.name));
    }
    Ok((out, ty, ended))
}

impl Walker<'_> {
    /// `steps` from a value of type `ty`: what each names, the type at the end, and why the
    /// path can go no further, when it cannot.
    fn walk(
        &self,
        mut ty: Ty,
        path: &[TieStep],
        depth: usize,
    ) -> Result<(Vec<Step>, Ty, Option<String>), Error> {
        let mut steps = Vec::new();
        // Set once a step names something a path cannot reach into, with why.
        let mut ended: Option<String> = None;
        // The value is a pointer held in a field, whose `[*]` is the memory behind it.
        let mut pointer_field = false;
        for step in path {
            let span = match step {
                TieStep::Field(f) | TieStep::Tie(f) => f.span,
                TieStep::Elements(s) => *s,
            };
            if let Some(why) = &ended {
                return Err(error(
                    codes::TIE_PATH_UNRESOLVED,
                    span,
                    format!("a tie path ends at {why}"),
                    "nothing further to name",
                    "end the path where it is; it already names all of that memory (spec §6.7)",
                ));
            }
            match step {
                TieStep::Field(f) => {
                    ty = self.field(&ty, &f.name, f.span)?;
                    pointer_field = matches!(ty, Ty::Ptr { .. } | Ty::OwnPtr { .. });
                    steps.push(Step::Field(f.name.clone()));
                }
                // `@home`, which no struct declares (D-0097): whether it may stand where it
                // does is the caller's to say, from the `Step::Home` it finds.
                TieStep::Tie(t) if t.name == "home" => {
                    if !owns_memory(self.table, &ty) {
                        let n = crate::type_name(self.res, ty.peeled());
                        return Err(error(
                            codes::HOME_MISPLACED,
                            t.span,
                            format!("`{n}` owns no memory, so it has no home"),
                            "nothing here has a home",
                            "`@home` names the region or heap a value that owns memory was placed \
                             in (D-0097)",
                        ));
                    }
                    ended = Some("`@home`, which names where the memory is".to_owned());
                    steps.push(Step::Home);
                }
                TieStep::Tie(t) => {
                    let parts = self.tie(&ty, &t.name, t.span, depth)?;
                    if let Some(why) = parts.iter().find_map(|(_, why)| why.clone()) {
                        ended = Some(format!("`@{}`, {why}", t.name));
                    } else if parts.iter().any(|(p, _)| *p != parts[0].0) {
                        // A tie standing for several places names all their memory at once;
                        // a path can go on into them only where there is one type to go on
                        // into.
                        ended =
                            Some(format!("`@{}`, which ties fields of different types", t.name));
                    }
                    ty = parts.into_iter().next().map(|(t, _)| t).unwrap_or(Ty::Error);
                    pointer_field = false;
                    steps.push(Step::Tie(t.name.clone()));
                }
                TieStep::Elements(s) => {
                    match (&ty, pointer_field) {
                        // A pointer field's `[*]` is the array it points at.
                        (Ty::Ptr { inner, .. } | Ty::OwnPtr { inner, .. }, true) => {
                            ty = (**inner).clone();
                        }
                        _ => match elements_in(self.table, &ty) {
                            Some(Some(elem)) => ty = elem,
                            Some(None) => {
                                let n = crate::type_name(self.res, ty.peeled());
                                ended = Some(format!("the entries of `{n}`"));
                            }
                            None => {
                                let n = crate::type_name(self.res, ty.peeled());
                                return Err(error(
                                    codes::TIE_PATH_UNRESOLVED,
                                    *s,
                                    format!("`{n}` has no elements"),
                                    "nothing for `[*]` to name",
                                    "`[*]` names the elements of an array, a slice, a pointer \
                                     field, or a type with an `Index` or `Iterable` \
                                     implementation (spec §6.7)",
                                ));
                            }
                        },
                    }
                    pointer_field = false;
                    steps.push(Step::Elements);
                }
            }
        }
        Ok((steps, ty, ended))
    }

    /// The type of `ty`'s field `name`, which the path's namer must be able to see.
    fn field(&self, ty: &Ty, name: &str, span: Span) -> Result<Ty, Error> {
        let n = crate::type_name(self.res, ty.peeled());
        let Ty::Named { sym, args } = ty.peeled() else {
            return Err(error(
                codes::TIE_PATH_UNRESOLVED,
                span,
                format!("`{n}` has no fields"),
                "no field to name",
                "a tie path goes through a struct's fields (spec §6.7)",
            ));
        };
        let fields = self.table.struct_sig(*sym).map(|s| s.fields.as_slice()).unwrap_or_default();
        let Some(f) = fields.iter().find(|f| f.name == name) else {
            return Err(error(
                codes::TIE_PATH_UNRESOLVED,
                span,
                format!("`{n}` has no field `{name}`"),
                "no such field",
                "a tie path goes through a struct's fields (spec §6.7)",
            ));
        };
        let owner = f.declared_in.unwrap_or(*sym);
        if !self.sees(owner, f.vis) {
            let owner_name = &self.res.symbol(owner).name;
            // A tie is a public name, so it drops a private field's `m_`.
            let tie_name = name.strip_prefix("m_").unwrap_or(name);
            let declare = format!(
                "have `{owner_name}` declare a tie for it, `tie @{tie_name} = {name}`, and name \
                 the tie instead (spec §6.7)"
            );
            return Err(match self.callers {
                Some(_) => error(
                    codes::TIE_PATH_NOT_VISIBLE,
                    span,
                    format!(
                        "`{owner_name}::{name}` is less visible than `{}`, whose callers must \
                         see it",
                        self.name
                    ),
                    "not visible to every caller",
                    format!(
                        "a clause names only what every caller of `{}` can see: make the field \
                         as visible as the function, or {declare}",
                        self.name
                    ),
                ),
                None => error(
                    codes::TIE_PATH_NOT_VISIBLE,
                    span,
                    format!("`{owner_name}::{name}` is private to `{owner_name}`"),
                    "not visible here",
                    format!(
                        "a tie names another type's memory through that type's ties: {declare}"
                    ),
                ),
            });
        }
        let field_ty = self.table.field_types(*sym, args, std::slice::from_ref(f)).remove(0);
        Ok(field_ty)
    }

    /// The type at the end of each path `ty`'s tie `@name` stands for, and why the path
    /// can go no further there, when it cannot. A path that does not resolve is reported
    /// on the declaration, by [`check_struct`], so here it is only an error type.
    fn tie(
        &self,
        ty: &Ty,
        name: &str,
        span: Span,
        depth: usize,
    ) -> Result<Vec<(Ty, Option<String>)>, Error> {
        let n = crate::type_name(self.res, ty.peeled());
        let mut found = Vec::new();
        if let Ty::Named { sym, args } = ty.peeled() {
            for t in tie_decls(self.table, *sym).iter().filter(|t| t.name.as_deref() == Some(name))
            {
                for path in &t.paths {
                    found.push(
                        export_path_at(self.table, self.res, *sym, args, path, depth + 1)
                            .map_or((Ty::Error, None), |(_, ty, why)| (ty, why)),
                    );
                }
            }
        }
        if found.is_empty() {
            return Err(error(
                codes::TIE_PATH_UNRESOLVED,
                span,
                format!("`{n}` declares no tie `@{name}`"),
                "no such tie",
                format!(
                    "a type declares a tie among its members, `tie @{name} = m_field`, saying \
                     what memory of its own the tie stands for (spec §6.7)"
                ),
            ));
        }
        Ok(found)
    }

    /// Whether a field of `owner` at `vis` is visible to whoever names the path: they must
    /// reach it, and in a signature it must be at least as visible as the function (§4).
    fn sees(&self, owner: SymbolId, vis: Vis) -> bool {
        let reachable = match vis {
            Vis::Public => true,
            Vis::Module => self.res.symbol(owner).module == self.module,
            Vis::Protected => {
                self.privileged.is_some_and(|p| p == owner || self.table.inherits_from(p, owner))
            }
            Vis::Private => self.privileged == Some(owner),
        };
        reachable && self.callers.map_or(true, |callers| rank(vis) >= rank(callers))
    }

    /// The same for an item of `module` at `vis`, which is `Public` or `Module`.
    fn sees_item(&self, module: ModuleId, vis: Vis) -> bool {
        let reachable = vis == Vis::Public || module == self.module;
        reachable && self.callers.map_or(true, |callers| rank(vis) >= rank(callers))
    }
}

/// The signature a call through a value of function type reads (spec §6.7): its
/// parameters, named as the type names them, its result and where it points, and its
/// clause, the most a function stored in it may kill. A pointer from C has no clause to read.
pub fn fn_type_sig(params: &[Ty], ret: &Ty, kills: &FnKills) -> FnSig {
    type Statics = [(Vec<String>, SymbolId)];
    let (names, results, paths, statics, unknown): (
        &[String],
        &[TiePath],
        &[TiePath],
        &Statics,
        bool,
    ) = match kills {
        FnKills::Known { names, results, paths, statics } => {
            (names, results, paths, statics, false)
        }
        FnKills::Unknown => (&[], &[], &[], &[], true),
    };
    FnSig {
        params: params
            .iter()
            .enumerate()
            .map(|(i, ty)| ParamSig {
                name: names.get(i).cloned().unwrap_or_default(),
                ty: ty.clone(),
                has_default: false,
                caller_default: None,
                c_cast: None,
                tie: None,
            })
            .collect(),
        ret: ret.clone(),
        fallible: false,
        takes_self: None,
        is_mut: false,
        is_unsafe: false,
        foreign_unsafe: None,
        statics: statics.to_vec(),
        // A type's clause names what anyone holding the type can see.
        vis: Vis::Public,
        span: Span::DUMMY,
        generics: Vec::new(),
        has_body: false,
        ret_cast: false,
        dispatch: dz_ast::Dispatch::Static,
        variadic: None,
        ret_ties: results.to_vec(),
        kills: paths.to_vec(),
        kills_span: None,
        kills_unknown: unknown,
        sends: Vec::new(),
        sends_span: None,
        ret_span: None,
        ret_home: Vec::new(),
    }
}

/// Every mistake in the clauses of the function types inside `ty`, written at `span`: a
/// path that names nothing, a kill through a parameter the function cannot change, an `in`
/// on a result that holds no view, and a result holding a view that does not say where it
/// points, as a declaration's must (spec §6.7).
pub fn check_fn_types(
    table: &TypeTable,
    res: &Resolution,
    module: ModuleId,
    ty: &Ty,
    span: Span,
) -> Vec<Error> {
    let mut out = Vec::new();
    fn_types(table, res, module, ty, span, &mut out, 0);
    out
}

fn fn_types(
    table: &TypeTable,
    res: &Resolution,
    module: ModuleId,
    ty: &Ty,
    span: Span,
    out: &mut Vec<Error>,
    depth: usize,
) {
    if depth > MAX_DEPTH {
        return;
    }
    let mut inner = |t: &Ty| fn_types(table, res, module, t, span, out, depth + 1);
    match ty {
        Ty::Fn { params, ret, kills } => {
            for p in params {
                inner(p);
            }
            inner(ret);
            let sig = fn_type_sig(params, ret, kills);
            let scope = Scope {
                table,
                res,
                module,
                name: "fn(…)",
                sig: &sig,
                self_ty: None,
                privileged: None,
            };
            for path in &sig.kills {
                match resolve(&scope, path) {
                    Ok(tie) => out.extend(changeable(&scope, &tie, path).err()),
                    Err(e) => out.push(e),
                }
            }
            for path in &sig.ret_ties {
                match resolve(&scope, path) {
                    Ok(_) if !holds_view(table, ret, true) => {
                        out.push(points_nowhere(&scope, "the result", ret, path.span))
                    }
                    Ok(_) => {}
                    Err(e) => out.push(e),
                }
            }
            if let FnKills::Known { names, .. } = kills {
                out.extend(untied_fn_type(&scope, names, span));
            }
        }
        Ty::Ref { inner: t, .. }
        | Ty::Ptr { inner: t, .. }
        | Ty::OwnPtr { inner: t, .. }
        | Ty::Optional(t)
        | Ty::Fallible(t)
        | Ty::Range(t)
        | Ty::Array { elem: t, .. }
        | Ty::Slice { elem: t, .. } => inner(t),
        Ty::Tuple(parts) => parts.iter().for_each(inner),
        Ty::Named { args, .. } => args.iter().for_each(inner),
        _ => {}
    }
}

/// The `tie` declarations of `sym` and of the types it inherits from, its own first. A
/// base's paths start at fields the derived type has too (spec §7.3).
pub fn tie_decls(table: &TypeTable, sym: SymbolId) -> Vec<TieSig> {
    let mut out = Vec::new();
    let mut at = Some(sym);
    // A cycle of bases is reported elsewhere; the bound keeps this from following one.
    for _ in 0..64 {
        let Some(s) = at.and_then(|a| table.struct_sig(a)) else { break };
        out.extend(s.ties.iter().cloned());
        at = s.base;
    }
    out
}

/// What `[*]` names in `ty`: `Some(Some(t))` for elements of type `t` — an array's, a
/// slice's, or what an `Index<usize>` implementation hands back — `Some(None)` for a type
/// that can only be iterated, whose elements a path cannot reach into, and `None` for a
/// type with no elements at all.
pub fn elements(scope: &Scope<'_>, ty: &Ty) -> Option<Option<Ty>> {
    elements_in(scope.table, ty)
}

/// [`elements`], with nothing but the table.
pub fn elements_in(table: &TypeTable, ty: &Ty) -> Option<Option<Ty>> {
    if let Some(e) = ty.element() {
        return Some(Some(e.clone()));
    }
    let t = ty.peeled();
    if !matches!(t, Ty::Named { .. }) {
        return None;
    }
    if let Some(index) = prelude_trait(table, "Index") {
        let r = TraitRef { trait_sym: index, args: vec![Ty::Int(IntTy::Usize)], span: Span::DUMMY };
        if table.implements(t, &r) {
            if let Some(sig) = table.trait_method(&r, t, "index") {
                return Some(Some(sig.ret.peeled_refs().clone()));
            }
        }
    }
    let iterable = prelude_trait(table, "Iterable")?;
    let r = TraitRef { trait_sym: iterable, args: Vec::new(), span: Span::DUMMY };
    table.implements(t, &r).then_some(None)
}

/// What a `for` over an `Iterable` `ty` hands out: its iterator's `get`. A `String`'s
/// characters are values computed from its bytes, so its `[*]` names no memory, while a
/// `HashMap`'s entries are references into its own.
pub fn iterated(scope: &Scope<'_>, ty: &Ty) -> Option<Ty> {
    let t = ty.peeled();
    let iterable = prelude_trait(scope.table, "Iterable")?;
    let r = TraitRef { trait_sym: iterable, args: Vec::new(), span: Span::DUMMY };
    let iter = scope.table.trait_method(&r, t, "iter")?.ret;
    let iterate = prelude_trait(scope.table, "Iterate")?;
    let r = TraitRef { trait_sym: iterate, args: Vec::new(), span: Span::DUMMY };
    Some(scope.table.trait_method(&r, &iter, "get")?.ret)
}

fn prelude_trait(table: &TypeTable, name: &str) -> Option<SymbolId> {
    table.prelude_names.iter().find(|(_, n)| n.as_str() == name).map(|(s, _)| *s)
}

fn rank(v: Vis) -> u8 {
    match v {
        Vis::Private => 0,
        Vis::Protected => 1,
        Vis::Module => 2,
        Vis::Public => 3,
    }
}
