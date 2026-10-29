//! Tie paths in signatures, resolved (spec §6.7).
//!
//! A signature keeps its `kills` clause and its `in` ties as written, since it is collected
//! before every type's fields are. [`resolve`] reads one once they are: which parameter it
//! starts from, and the field, element or tie each step names. The checker reports what
//! [`check`] finds wrong, once per signature; `dz_flow` calls [`resolve`] for the paths
//! themselves, which by then all resolve.

use dz_ast::{TiePath, TieStep, Vis};
use dz_diag::codes;
use dz_hir::ty::{IntTy, Ty};
use dz_project::ModuleId;
use dz_resolve::{Resolution, SymbolId};
use dz_span::Span;

use crate::{FnSig, TraitRef, TypeTable};

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
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Step {
    Field(String),
    /// `[*]`
    Elements,
    /// `@items`
    Tie(String),
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

/// Every mistake in `scope`'s signature: a path that names nothing or something its
/// callers cannot see, a `kills` through something the function cannot change, and an
/// `in` on something that points into nothing. Reported once each.
pub fn check(scope: &Scope<'_>, params: &[dz_ast::Param]) -> Vec<Error> {
    let mut out = Vec::new();
    for path in &scope.sig.kills {
        match resolve(scope, path) {
            Ok(tie) => {
                if let Err(e) = changeable(scope, &tie, path) {
                    out.push(e);
                }
            }
            Err(e) => out.push(e),
        }
    }
    if let Some(path) = &scope.sig.ret_tie {
        match resolve(scope, path) {
            Ok(_) if !points(&scope.sig.ret) => {
                out.push(points_nowhere(scope, "the result", &scope.sig.ret, path.span))
            }
            Ok(_) => {}
            Err(e) => out.push(e),
        }
    }
    for (i, (p, sig)) in params.iter().zip(&scope.sig.params).enumerate() {
        let Some(path) = &p.tie else { continue };
        match resolve(scope, path) {
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

/// Whether a `T` points into memory something else owns: a reference, a pointer, a slice
/// or a `str`.
fn points(t: &Ty) -> bool {
    matches!(t, Ty::Ref { .. } | Ty::Ptr { .. } | Ty::Slice { .. } | Ty::Str | Ty::CStr)
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

/// A `kills` path must start where the function can change something: a `&mut` or `^mut`
/// parameter, or `&mut self`. What it was given by value is its own, and its callers never
/// see that die.
fn changeable(scope: &Scope<'_>, tie: &Tie, path: &TiePath) -> Result<(), Error> {
    let (what, ty) = match tie.root {
        Root::SelfValue => {
            if scope.sig.takes_self == Some(true) {
                return Ok(());
            }
            ("`self`".to_owned(), "taken as `&self`".to_owned())
        }
        Root::Param(i) => {
            let p = &scope.sig.params[i];
            if matches!(p.ty, Ty::Ref { mutable: true, .. } | Ty::Ptr { mutable: true, .. }) {
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
    let (root, mut ty) = if path.root.name == "self" {
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
        match scope.sig.params.iter().position(|p| p.name == path.root.name) {
            Some(i) => (Root::Param(i), scope.sig.params[i].ty.clone()),
            None => {
                return Err(error(
                    codes::TIE_PATH_UNRESOLVED,
                    path.root.span,
                    format!("`{}` is not a parameter of `{}`", path.root.name, scope.name),
                    "no parameter of this name",
                    "a tie path starts at one of the function's parameters, or at `self` in a \
                     method (spec §6.7)",
                ))
            }
        }
    };

    let mut steps = Vec::new();
    // Set once a step names something a path cannot reach into, with why.
    let mut ended: Option<String> = None;
    for step in &path.steps {
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
                ty = field(scope, &ty, &f.name, f.span)?;
                steps.push(Step::Field(f.name.clone()));
            }
            TieStep::Tie(t) => {
                let fields = tie(scope, &ty, &t.name, t.span)?;
                if let Some((_, f)) =
                    fields.iter().find(|(t, _)| matches!(t, Ty::Ptr { .. } | Ty::OwnPtr { .. }))
                {
                    ended = Some(format!("`@{}`, the memory `{f}` points into", t.name));
                } else if fields.iter().any(|(t, _)| *t != fields[0].0) {
                    // Fields tied together name all their memory at once; a path can go on
                    // into them only where there is one type to go on into.
                    ended = Some(format!("`@{}`, which ties fields of different types", t.name));
                }
                ty = fields.into_iter().next().map(|(t, _)| t).unwrap_or(Ty::Error);
                steps.push(Step::Tie(t.name.clone()));
            }
            TieStep::Elements(s) => {
                match elements(scope, &ty) {
                    Some(Some(elem)) => ty = elem,
                    Some(None) => {
                        let n = crate::type_name(scope.res, ty.peeled());
                        ended = Some(format!("the entries of `{n}`"));
                    }
                    None => {
                        let n = crate::type_name(scope.res, ty.peeled());
                        return Err(error(
                            codes::TIE_PATH_UNRESOLVED,
                            *s,
                            format!("`{n}` has no elements"),
                            "nothing for `[*]` to name",
                            "`[*]` names the elements of an array, a slice, or a type with an \
                             `Index` or `Iterable` implementation (spec §6.7)",
                        ));
                    }
                }
                steps.push(Step::Elements);
            }
        }
    }
    Ok(Tie { root, steps })
}

/// The type of `ty`'s field `name`, which every caller of the function must be able to see.
fn field(scope: &Scope<'_>, ty: &Ty, name: &str, span: Span) -> Result<Ty, Error> {
    let n = crate::type_name(scope.res, ty.peeled());
    let Ty::Named { sym, args } = ty.peeled() else {
        return Err(error(
            codes::TIE_PATH_UNRESOLVED,
            span,
            format!("`{n}` has no fields"),
            "no field to name",
            "a tie path goes through a struct's fields (spec §6.7)",
        ));
    };
    let fields = scope.table.struct_sig(*sym).map(|s| s.fields.as_slice()).unwrap_or_default();
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
    if !visible_to_callers(scope, owner, f.vis) {
        let owner_name = &scope.res.symbol(owner).name;
        // A tie is a public name, so it drops a private field's `m_`.
        let tie_name = name.strip_prefix("m_").unwrap_or(name);
        return Err(error(
            codes::TIE_PATH_NOT_VISIBLE,
            span,
            format!(
                "`{owner_name}::{name}` is less visible than `{}`, whose callers must see it",
                scope.name
            ),
            "not visible to every caller",
            format!(
                "a clause names only what every caller of `{}` can see: make the field as \
                 visible as the function, or declare a tie on it, `{name}: … in @{tie_name}`, \
                 and name the tie instead (spec §6.7)",
                scope.name
            ),
        ));
    }
    let field_ty = scope.table.field_types(*sym, args, std::slice::from_ref(f)).remove(0);
    Ok(field_ty)
}

/// The field declaring `ty`'s tie `@name`, by its type and name. Ties are public names:
/// declaring one is how a type lets its callers name private storage.
/// Every field of `ty` that declares the tie `name`, with its type: fields may be tied
/// together, as a `HashMap`'s arrays are (spec §6.7).
fn tie(scope: &Scope<'_>, ty: &Ty, name: &str, span: Span) -> Result<Vec<(Ty, String)>, Error> {
    let n = crate::type_name(scope.res, ty.peeled());
    let found = match ty.peeled() {
        Ty::Named { sym, args } => scope.table.struct_sig(*sym).map(|s| {
            let fields: Vec<_> =
                s.fields.iter().filter(|f| f.tie.as_deref() == Some(name)).cloned().collect();
            let types = scope.table.field_types(*sym, args, &fields);
            types.into_iter().zip(fields.into_iter().map(|f| f.name)).collect::<Vec<_>>()
        }),
        _ => None,
    };
    found.filter(|fields| !fields.is_empty()).ok_or_else(|| {
        error(
            codes::TIE_PATH_UNRESOLVED,
            span,
            format!("`{n}` declares no tie `@{name}`"),
            "no such tie",
            format!(
                "a type declares a tie by writing `in @{name}` after one of its fields' types \
                 (spec §6.7)"
            ),
        )
    })
}

/// What `[*]` names in `ty`: `Some(Some(t))` for elements of type `t` — an array's, a
/// slice's, or what an `Index<usize>` implementation hands back — `Some(None)` for a type
/// that can only be iterated, whose elements a path cannot reach into, and `None` for a
/// type with no elements at all.
pub fn elements(scope: &Scope<'_>, ty: &Ty) -> Option<Option<Ty>> {
    if let Some(e) = ty.element() {
        return Some(Some(e.clone()));
    }
    let t = ty.peeled();
    if !matches!(t, Ty::Named { .. }) {
        return None;
    }
    if let Some(index) = prelude_trait(scope.table, "Index") {
        let r = TraitRef { trait_sym: index, args: vec![Ty::Int(IntTy::Usize)], span: Span::DUMMY };
        if scope.table.implements(t, &r) {
            if let Some(sig) = scope.table.trait_method(&r, t, "index") {
                return Some(Some(sig.ret.peeled_refs().clone()));
            }
        }
    }
    let iterable = prelude_trait(scope.table, "Iterable")?;
    let r = TraitRef { trait_sym: iterable, args: Vec::new(), span: Span::DUMMY };
    scope.table.implements(t, &r).then_some(None)
}

fn prelude_trait(table: &TypeTable, name: &str) -> Option<SymbolId> {
    table.prelude_names.iter().find(|(_, n)| n.as_str() == name).map(|(s, _)| *s)
}

/// Whether a field of `owner` at `vis` is visible everywhere the function can be called
/// from: the function itself must reach it, and it must be at least as visible as the
/// function. A free function's own privacy is the module's, since anything in the module
/// may call it (spec §4).
fn visible_to_callers(scope: &Scope<'_>, owner: SymbolId, vis: Vis) -> bool {
    let reachable = match vis {
        Vis::Public => true,
        Vis::Module => scope.res.symbol(owner).module == scope.module,
        Vis::Protected => {
            scope.privileged.is_some_and(|p| p == owner || scope.table.inherits_from(p, owner))
        }
        Vis::Private => scope.privileged == Some(owner),
    };
    let fn_vis = match (scope.privileged, scope.sig.vis) {
        (None, Vis::Private) => Vis::Module,
        (_, v) => v,
    };
    reachable && rank(vis) >= rank(fn_vis)
}

fn rank(v: Vis) -> u8 {
    match v {
        Vis::Private => 0,
        Vis::Protected => 1,
        Vis::Module => 2,
        Vis::Public => 3,
    }
}
