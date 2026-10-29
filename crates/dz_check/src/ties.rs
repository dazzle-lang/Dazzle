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
use dz_hir::ty::{IntTy, Ty};
use dz_project::ModuleId;
use dz_resolve::{Resolution, SymbolId};
use dz_span::Span;

use crate::{FnSig, TieSig, TraitRef, TypeTable};

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
    let (steps, _, _) = Walker::of(scope).walk(ty, &path.steps, 0)?;
    Ok(Tie { root, steps })
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
