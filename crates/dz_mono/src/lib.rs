//! Monomorphization (spec §8.4, §17.1).
//!
//! Generic bodies were checked once, at their definition, against their bounds (§8.1).
//! What remains is to decide which instantiations exist and to produce a concrete copy of
//! each body for codegen — and that is all this crate does. It reports nothing about the
//! bodies themselves: a mistake inside a generic was reported where it was written, and
//! reporting it again per instantiation is exactly what §8.6 rules out.
//!
//! The copies are ordinary [`Body`] values with every type substituted, so codegen emits an
//! instance with the same code it uses for any other body and never has to know it is
//! looking at one. The single diagnostic this crate can produce is DZ0536, for
//! instantiation that would never end.
//!
//! Instances are collected **per module**: each module gets every instantiation reachable
//! from its own non-generic code. Two modules that both use `Buffer<f32>` therefore both
//! list it, and codegen emits it once per header under an include guard in the generic's
//! own namespace, so the two agree on one type.
//!
//! Four things are instantiated: generic types, generic functions, generic implementations
//! (`impl<T: Inspect> Inspect for Buffer<T>`, spec §7.6) — because an implementation's
//! methods are free functions of their own in the output — and methods with type parameters
//! of their own (`fn fold<A>(&self, …)`, §8.1), one per call's arguments.

use std::collections::HashMap;

use dz_check::{TraitRef, TypeTable};
use dz_diag::{codes, Diagnostic, Diagnostics};
use dz_hir::ty::{self, Ty};
use dz_hir::{Body, BodyOwner, Callee, ExprKind, Hir, StmtKind};
use dz_project::ModuleId;
use dz_resolve::{Resolution, SymbolId, SymbolKind};
use dz_span::{FileId, Span};

/// How deeply type arguments may nest before instantiation is taken not to terminate.
///
/// `Buffer<Buffer<f32>>` is depth 2, and real programs stay in single digits. The bound is
/// the compiler's rather than C++'s template depth, so the error names Dazzle code and the
/// line that caused it (spec §8.4).
pub const MAX_DEPTH: usize = 32;

/// One generic declaration applied to concrete arguments, with the concrete copy of every
/// body it needs: a function's one body, or all of a type's members.
pub struct Instance {
    pub sym: SymbolId,
    pub args: Vec<Ty>,
    pub bodies: Vec<Body>,
}

impl Instance {
    pub fn find(&self, owner: &BodyOwner) -> Option<&Body> {
        self.bodies.iter().find(|b| &b.owner == owner)
    }
}

/// One generic implementation applied to concrete arguments — one per parameter of the
/// `impl`, in declaration order — with its methods and the trait's defaults it inherits.
pub struct ImplInstance {
    pub imp: usize,
    pub args: Vec<Ty>,
    pub bodies: Vec<Body>,
}

impl ImplInstance {
    pub fn find(&self, owner: &BodyOwner) -> Option<&Body> {
        self.bodies.iter().find(|b| &b.owner == owner)
    }
}

/// A method with type parameters of its own, `fn map<U>(&self, …)`, applied to concrete
/// arguments: the type's (`owner_args`, empty unless the type is generic) and the method's
/// (`args`). One body, owned by the method as the template is (spec §8.1, §8.4).
pub struct MethodInstance {
    pub owner: SymbolId,
    pub owner_args: Vec<Ty>,
    pub name: String,
    pub args: Vec<Ty>,
    pub body: Body,
}

#[derive(Default)]
pub struct ModuleInstances {
    /// Generic structs, in discovery order. Codegen orders them by layout dependency.
    pub types: Vec<Instance>,
    pub fns: Vec<Instance>,
    pub impls: Vec<ImplInstance>,
    pub methods: Vec<MethodInstance>,
    /// The trait defaults each of the module's non-generic implementations inherits rather
    /// than writing (spec §7.6), one concrete body each, owned by the implementation.
    pub defaults: Vec<Body>,
}

pub struct Mono {
    pub by_module: HashMap<ModuleId, ModuleInstances>,
    pub diagnostics: Diagnostics,
}

impl Mono {
    pub fn module(&self, m: ModuleId) -> Option<&ModuleInstances> {
        self.by_module.get(&m)
    }
}

/// Collect every instantiation each module needs (spec §8.4).
pub fn monomorphize(res: &Resolution, table: &TypeTable, hir: &Hir) -> Mono {
    // A type's member bodies, by type, and an implementation's, by implementation, so an
    // instance can find all of them at once.
    let mut members: HashMap<SymbolId, Vec<&Body>> = HashMap::new();
    let mut impl_members: HashMap<usize, Vec<&Body>> = HashMap::new();
    for (_, b) in hir.bodies() {
        match &b.owner {
            // A trait's default arguments are read by the trait object's handle, never
            // instantiated as a member.
            BodyOwner::Fn(_)
            | BodyOwner::Const(_)
            | BodyOwner::Static(_)
            | BodyOwner::TraitDefaults { .. }
            | BodyOwner::AssocConst { .. } => {}
            BodyOwner::ImplMethod { imp, .. } => {
                impl_members.entry(*imp as usize).or_default().push(b)
            }
            other => {
                if let Some(sym) = owner_sym(other) {
                    members.entry(sym).or_default().push(b);
                }
            }
        }
    }

    let mut by_module = HashMap::new();
    let mut diagnostics = Diagnostics::new();
    let modules: Vec<ModuleId> = {
        let mut ms: Vec<ModuleId> = res.symbols.iter().map(|s| s.module).collect();
        ms.sort_by_key(|m| m.0);
        ms.dedup();
        ms
    };

    for m in modules {
        let mut w = Worklist {
            table,
            hir,
            res,
            members: &members,
            impl_members: &impl_members,
            out: ModuleInstances::default(),
            requested: Vec::new(),
            requested_impls: Vec::new(),
            requested_methods: Vec::new(),
            queue: Vec::new(),
            diags: Diagnostics::new(),
            too_deep: false,
        };

        // Seeds: every concrete body the module owns...
        for (_, b) in hir.bodies() {
            if owner_module(res, table, &b.owner) == m && !is_template(table, &b.owner) {
                w.visit_body(b);
            }
        }
        // ...the defaults its concrete implementations inherit...
        for (idx, imp) in table.impls().iter().enumerate() {
            if imp.module == m && imp.generics.is_empty() {
                for b in impl_defaults(table, hir, idx, &HashMap::new()) {
                    w.visit_body(&b);
                    w.out.defaults.push(b);
                }
            }
        }
        // ...and every field of every concrete type it declares, since a struct holding a
        // `Buffer<f32>` needs that instance declared whether or not any body touches it.
        for (i, s) in res.symbols.iter().enumerate() {
            let sym = SymbolId(i as u32);
            if s.module != m || table.is_generic(sym) {
                continue;
            }
            let at = (s.file, s.span);
            match s.kind {
                SymbolKind::Struct => {
                    if let Some(sig) = table.struct_sig(sym) {
                        for f in &sig.fields {
                            w.visit_ty(&f.ty, at);
                        }
                        // `Button : Panel<i32>` holds a `Panel<i32>` as its first part.
                        if let Some(base) = sig.base {
                            w.visit_ty(&Ty::Named { sym: base, args: sig.base_args.clone() }, at);
                        }
                    }
                }
                SymbolKind::Union => {
                    if let Some(dz_check::ItemSig::Union { variants, .. }) = table.get(sym) {
                        for v in variants {
                            for f in &v.fields {
                                w.visit_ty(&f.ty, at);
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        w.drain();
        diagnostics.extend(w.diags);
        let o = &w.out;
        if !o.types.is_empty()
            || !o.fns.is_empty()
            || !o.impls.is_empty()
            || !o.methods.is_empty()
            || !o.defaults.is_empty()
        {
            by_module.insert(m, w.out);
        }
    }

    Mono { by_module, diagnostics }
}

/// The declaration a body belongs to — `None` for an implementation's method, since an
/// implementation has no name.
pub fn owner_sym(owner: &BodyOwner) -> Option<SymbolId> {
    match owner {
        BodyOwner::Fn(s)
        | BodyOwner::Copy(s)
        | BodyOwner::Drop(s)
        | BodyOwner::Const(s)
        | BodyOwner::Static(s) => Some(*s),
        BodyOwner::Method { owner, .. }
        | BodyOwner::Ctor { owner, .. }
        | BodyOwner::FieldDefault { owner, .. }
        | BodyOwner::TraitDefaults { owner, .. }
        | BodyOwner::AssocConst { owner, .. } => Some(*owner),
        BodyOwner::ImplMethod { .. } => None,
    }
}

/// The module a body is written in.
pub fn owner_module(res: &Resolution, table: &TypeTable, owner: &BodyOwner) -> ModuleId {
    match (owner_sym(owner), owner) {
        (Some(s), _) => res.symbol(s).module,
        (None, BodyOwner::ImplMethod { imp, .. }) => table.impls()[*imp as usize].module,
        (None, _) => ModuleId(0),
    }
}

/// A body is a template when it mentions a parameter nobody has chosen: a generic
/// function's, a generic type's member, a generic implementation's method, or a trait's
/// default method (generic in `Self`). Only instances of these are ever emitted.
pub fn is_template(table: &TypeTable, owner: &BodyOwner) -> bool {
    if let BodyOwner::ImplMethod { imp, .. } = owner {
        return !table.impls()[*imp as usize].generics.is_empty();
    }
    let Some(sym) = owner_sym(owner) else { return false };
    if table.is_generic(sym) {
        return true;
    }
    is_generic_method(table, owner)
}

/// A type's method with type parameters of its own (spec §8.1): a template even in a
/// non-generic type, and instantiated per call rather than with its type.
pub fn is_generic_method(table: &TypeTable, owner: &BodyOwner) -> bool {
    let BodyOwner::Method { owner, name } = owner else { return false };
    table.methods_sig(*owner).and_then(|s| s.methods.get(name)).is_some_and(|m| m.is_generic())
}

enum Pending {
    Type(SymbolId, Vec<Ty>),
    Fn(SymbolId, Vec<Ty>),
    Impl(usize, Vec<Ty>),
    Method { owner: SymbolId, owner_args: Vec<Ty>, name: String, args: Vec<Ty> },
}

struct Worklist<'a> {
    table: &'a TypeTable,
    hir: &'a Hir,
    res: &'a Resolution,
    members: &'a HashMap<SymbolId, Vec<&'a Body>>,
    impl_members: &'a HashMap<usize, Vec<&'a Body>>,
    out: ModuleInstances,
    /// Everything ever asked for, so each instance is produced once. `Ty` has no `Hash`,
    /// and a module has few instances, so a list is the simple answer.
    requested: Vec<(bool, SymbolId, Vec<Ty>)>,
    requested_impls: Vec<(usize, Vec<Ty>)>,
    requested_methods: Vec<(SymbolId, Vec<Ty>, String, Vec<Ty>)>,
    queue: Vec<Pending>,
    diags: Diagnostics,
    /// Set after the first DZ0536, so one runaway instantiation is one diagnostic.
    too_deep: bool,
}

impl Worklist<'_> {
    fn drain(&mut self) {
        while let Some(p) = self.queue.pop() {
            match p {
                Pending::Type(sym, args) => self.instantiate_type(sym, args),
                Pending::Fn(sym, args) => self.instantiate_fn(sym, args),
                Pending::Impl(idx, args) => self.instantiate_impl(idx, args),
                Pending::Method { owner, owner_args, name, args } => {
                    self.instantiate_method(owner, owner_args, name, args)
                }
            }
        }
    }

    fn instantiate_method(
        &mut self,
        owner: SymbolId,
        owner_args: Vec<Ty>,
        name: String,
        args: Vec<Ty>,
    ) {
        let key = BodyOwner::Method { owner, name: name.clone() };
        let Some(template) = self.hir.find(&key) else { return };
        let Some(sig) = self.table.methods_sig(owner).and_then(|s| s.methods.get(&name)) else {
            return;
        };
        let mut map = self.params_map(owner, &owner_args);
        map.extend(sig.generics.iter().map(|g| g.param.id).zip(args.iter().cloned()));
        let body = table_body(self.table, template, &map, None);
        self.visit_body(&body);
        self.out.methods.push(MethodInstance { owner, owner_args, name, args, body });
    }

    /// A call of a generic method with concrete arguments (spec §8.4).
    fn want_method(
        &mut self,
        owner: SymbolId,
        owner_args: Vec<Ty>,
        name: &str,
        args: &[Ty],
        at: (FileId, Span),
    ) {
        if owner_args.iter().chain(args).any(Ty::has_params) {
            return; // reached again, concretely, through each instance
        }
        let wanted = (owner, owner_args, name.to_owned(), args.to_vec());
        if self.requested_methods.contains(&wanted) {
            return;
        }
        if wanted.1.iter().chain(args).map(depth).max().unwrap_or(0) >= MAX_DEPTH {
            self.too_deep(format!("{}::{name}", self.res.symbol(owner).name), at);
            return;
        }
        for a in wanted.1.iter().chain(args) {
            self.visit_ty(a, at);
        }
        self.requested_methods.push(wanted.clone());
        let (owner, owner_args, name, args) = wanted;
        self.queue.push(Pending::Method { owner, owner_args, name, args });
    }

    fn params_map(&self, sym: SymbolId, args: &[Ty]) -> HashMap<u32, Ty> {
        self.table.type_params(sym).iter().map(|p| p.param.id).zip(args.iter().cloned()).collect()
    }

    fn instantiate_type(&mut self, sym: SymbolId, args: Vec<Ty>) {
        let map = self.params_map(sym, &args);
        let self_ty = Ty::Named { sym, args: args.clone() };
        let copyable = self.table.copyable(&self_ty);
        let mut bodies = Vec::new();
        let members: Vec<&Body> = self.members.get(&sym).cloned().unwrap_or_default();
        for b in members {
            // A method with parameters of its own is instantiated per call, with this
            // instance's arguments and the call's (`instantiate_method`).
            if is_generic_method(self.table, &b.owner) {
                continue;
            }
            // `Drop` and `Copy` are implementations, written in terms of the implementation's
            // own parameters, so they are read through it rather than through the struct's
            // (spec §6.1). An instance its `impl Copy` does not cover has no `copy` at all.
            let special = match b.owner {
                BodyOwner::Drop(_) => Some(self.table.drop_impl(sym)),
                BodyOwner::Copy(_) if !copyable => continue,
                BodyOwner::Copy(_) => Some(self.table.copy_impl(sym)),
                _ => None,
            };
            let concrete = match special {
                Some(Some(idx)) => {
                    let imp = &self.table.impls()[idx];
                    let m = self.table.impl_applies(imp, &self_ty).unwrap_or_default();
                    table_body(self.table, b, &m, None)
                }
                _ => table_body(self.table, b, &map, None),
            };
            self.visit_body(&concrete);
            bodies.push(concrete);
        }
        // Fields matter even with no body: `Pair<Buffer<f32>>` needs `Buffer<f32>`.
        let s = self.res.symbol(sym);
        let at = (s.file, s.span);
        if let Some(sig) = self.table.struct_sig(sym) {
            for t in self.table.field_types(sym, &args, &sig.fields) {
                self.visit_ty(&t, at);
            }
            // `Slider<f32> : Panel<f32>` needs that instance of its base (T-0035).
            if let Some(base) = sig.base {
                let base_args = self.table.args_as(sym, &args, base);
                self.visit_ty(&Ty::Named { sym: base, args: base_args }, at);
            }
        }
        self.out.types.push(Instance { sym, args, bodies });
    }

    fn instantiate_fn(&mut self, sym: SymbolId, args: Vec<Ty>) {
        let Some(body) = self.hir.find(&BodyOwner::Fn(sym)) else { return };
        let map = self.params_map(sym, &args);
        let concrete = table_body(self.table, body, &map, None);
        self.visit_body(&concrete);
        self.out.fns.push(Instance { sym, args, bodies: vec![concrete] });
    }

    fn instantiate_impl(&mut self, idx: usize, args: Vec<Ty>) {
        let imp = &self.table.impls()[idx];
        let map: HashMap<u32, Ty> =
            imp.generics.iter().map(|g| g.param.id).zip(args.iter().cloned()).collect();
        let mut bodies = Vec::new();
        for b in self.impl_members.get(&idx).cloned().unwrap_or_default() {
            let concrete = table_body(self.table, b, &map, None);
            self.visit_body(&concrete);
            bodies.push(concrete);
        }
        for b in impl_defaults(self.table, self.hir, idx, &map) {
            self.visit_body(&b);
            bodies.push(b);
        }
        self.out.impls.push(ImplInstance { imp: idx, args, bodies });
    }

    /// Everything a concrete body mentions: the types of its locals and expressions, the
    /// generic functions it calls, and the generic implementations its calls reach.
    fn visit_body(&mut self, b: &Body) {
        let at = (b.file, b.span);
        self.visit_ty(&b.ret, at);
        for l in &b.locals {
            self.visit_ty(&l.ty, (b.file, l.span));
        }
        for e in &b.exprs {
            let here = (b.file, e.span);
            self.visit_ty(&e.ty, here);
            match &e.kind {
                ExprKind::TypeQuery { ty, .. } | ExprKind::Reflect { ty, .. } => {
                    self.visit_ty(ty, here)
                }
                // Called, or taken as a value: either way the instance must exist (§8.4).
                ExprKind::Call { callee: Callee::Fn { sym, type_args }, .. }
                | ExprKind::FnItem { sym, type_args }
                    if !type_args.is_empty() =>
                {
                    for t in type_args {
                        self.visit_ty(t, here);
                    }
                    if type_args.iter().all(|t| !t.has_params()) {
                        self.want(false, *sym, type_args.clone(), here);
                    }
                }
                // `Buffer<f32>::with_capacity(8)` needs the instance whose function it is,
                // whatever the call returns (spec §7.3, §8.4).
                ExprKind::Call {
                    callee: Callee::Assoc { owner, type_args, name, method_args },
                    ..
                } => {
                    if !type_args.is_empty() {
                        self.visit_ty(&Ty::Named { sym: *owner, args: type_args.clone() }, here);
                    }
                    if !method_args.is_empty() {
                        self.want_method(*owner, type_args.clone(), name, method_args, here);
                    }
                }
                // A trait's method on a concrete receiver reaches one implementation, which
                // may be generic (spec §7.6).
                ExprKind::Call { callee: Callee::Method { recv, owner, name, .. }, args }
                    if self.res.symbol(*owner).kind == SymbolKind::Trait =>
                {
                    let recv_ty = b.ty(*recv).peeled().clone();
                    let arg = args.first().map(|a| b.ty(*a).clone());
                    self.want_impl(*owner, &recv_ty, name, arg.as_ref(), Some(&e.ty), here);
                }
                // A type's own generic method: the type's arguments come from the receiver,
                // unless the method is a base's, which is never generic (DZ0541).
                ExprKind::Call {
                    callee: Callee::Method { recv, owner, name, method_args },
                    ..
                } if !method_args.is_empty() => {
                    let owner_args = match b.ty(*recv).peeled() {
                        Ty::Named { sym, args } if sym == owner => args.clone(),
                        _ => Vec::new(),
                    };
                    self.want_method(*owner, owner_args, name, method_args, here);
                }
                _ => {}
            }
        }
        // `for x in it` calls the iterator's `valid`, `get` and `advance` (spec §10.4).
        if let Some(iterate) = self.table.prelude_trait_named("Iterate") {
            for s in &b.stmts {
                if let StmtKind::For { subject, .. } = &s.kind {
                    let t = b.ty(*subject).peeled().clone();
                    self.want_impl(iterate, &t, "valid", None, None, (b.file, s.span));
                }
            }
        }
    }

    fn want_impl(
        &mut self,
        trait_sym: SymbolId,
        recv: &Ty,
        method: &str,
        arg: Option<&Ty>,
        ret: Option<&Ty>,
        at: (FileId, Span),
    ) {
        if recv.has_params() {
            return; // reached again, concretely, through each instance
        }
        let Some((idx, map)) = self.table.resolve_impl(trait_sym, recv, method, arg, ret) else {
            return;
        };
        let imp = &self.table.impls()[idx];
        if imp.generics.is_empty() {
            return; // emitted with the module that declares it
        }
        let args: Vec<Ty> = imp
            .generics
            .iter()
            .map(|g| map.get(&g.param.id).cloned().unwrap_or(Ty::Error))
            .collect();
        if self.requested_impls.iter().any(|(i, a)| *i == idx && *a == args) {
            return;
        }
        if args.iter().map(depth).max().unwrap_or(0) >= MAX_DEPTH {
            self.too_deep(self.res.symbol(trait_sym).name.clone(), at);
            return;
        }
        for a in &args {
            self.visit_ty(a, at);
        }
        self.requested_impls.push((idx, args.clone()));
        self.queue.push(Pending::Impl(idx, args));
    }

    fn visit_ty(&mut self, t: &Ty, at: (FileId, Span)) {
        match t {
            Ty::Named { sym, args } if !args.is_empty() => {
                for a in args {
                    self.visit_ty(a, at);
                }
                // A body reached only through a template still mentions its parameters;
                // it will be visited again, concretely, through each instance.
                if args.iter().all(|a| !a.has_params()) {
                    self.want(true, *sym, args.clone(), at);
                }
            }
            Ty::Optional(i) | Ty::Fallible(i) | Ty::Range(i) => self.visit_ty(i, at),
            Ty::Ref { inner, .. } | Ty::Ptr { inner, .. } | Ty::OwnPtr { inner, .. } => {
                self.visit_ty(inner, at)
            }
            Ty::Array { elem, .. } | Ty::Slice { elem, .. } => self.visit_ty(elem, at),
            Ty::Tuple(parts) => {
                for p in parts {
                    self.visit_ty(p, at);
                }
            }
            Ty::Fn { params, ret, .. } => {
                for p in params {
                    self.visit_ty(p, at);
                }
                self.visit_ty(ret, at);
            }
            _ => {}
        }
    }

    fn want(&mut self, is_type: bool, sym: SymbolId, args: Vec<Ty>, at: (FileId, Span)) {
        if self.requested.iter().any(|(t, s, a)| *t == is_type && *s == sym && *a == args) {
            return;
        }
        if args.iter().map(depth).max().unwrap_or(0) >= MAX_DEPTH {
            self.too_deep(self.res.symbol(sym).name.clone(), at);
            return;
        }
        self.requested.push((is_type, sym, args.clone()));
        self.queue.push(if is_type { Pending::Type(sym, args) } else { Pending::Fn(sym, args) });
    }

    fn too_deep(&mut self, name: String, at: (FileId, Span)) {
        if self.too_deep {
            return;
        }
        self.too_deep = true;
        self.diags.push(
            Diagnostic::error(
                codes::RECURSIVE_INSTANTIATION,
                format!("instantiating `{name}` does not terminate"),
            )
            .at(at.0, at.1, "each instantiation here asks for a deeper one")
            .with_help(format!(
                "type arguments nested more than {MAX_DEPTH} deep; a generic that instantiates \
                 itself with a larger type never reaches a concrete end (spec §8.4)"
            )),
        );
    }
}

/// The trait defaults an implementation inherits rather than writing (spec §7.6), as
/// concrete bodies owned by the implementation. `map` gives the implementation's own
/// parameters, empty for a non-generic one.
///
/// The trait's body was checked once, with `Self` unknown. Here `Self` becomes the type
/// implemented for, the trait's parameters become the implementation's arguments, and its
/// associated types become the implementation's choices — the same three substitutions a
/// call through the trait already sees. Methods are taken in declaration order so the
/// output is deterministic.
pub fn impl_defaults(
    table: &TypeTable,
    hir: &Hir,
    idx: usize,
    map: &HashMap<u32, Ty>,
) -> Vec<Body> {
    let imp = &table.impls()[idx];
    let self_ty = table.subst(&imp.self_ty, map);
    let r = TraitRef {
        trait_sym: imp.trait_ref.trait_sym,
        args: imp.trait_ref.args.iter().map(|a| table.subst_with(a, map, Some(&self_ty))).collect(),
        span: imp.trait_ref.span,
    };
    let Some(spec) = table.struct_sig(r.trait_sym) else { return Vec::new() };
    let mut out = Vec::new();
    for name in &spec.method_keys {
        if !spec.methods[name].has_body || imp.methods.contains_key(name) {
            continue;
        }
        let Some(template) =
            hir.find(&BodyOwner::Method { owner: r.trait_sym, name: name.clone() })
        else {
            continue;
        };
        let mut tmap: HashMap<u32, Ty> = table
            .type_params(r.trait_sym)
            .iter()
            .map(|p| p.param.id)
            .zip(r.args.iter().cloned())
            .collect();
        tmap.extend(table.assoc_bindings(&r, &self_ty));
        let body = table_body(table, template, &tmap, Some(&self_ty));
        // Inside its own default body the checker reads `self` through the trait, so
        // `Self` there is spelled as the trait applied to its arguments; the copy says the
        // implementer instead.
        let as_trait = Ty::Named { sym: r.trait_sym, args: r.args.clone() };
        let mut body = dz_hir::map_body_types(&body, |t| {
            table.normalize(&ty::replace(t, &as_trait, &self_ty))
        });
        body.owner = BodyOwner::ImplMethod { imp: idx as u32, name: name.clone() };
        out.push(body);
    }
    out
}

/// A concrete copy of `body`: every parameter in `map` substituted, and then every
/// projection that substitution made concrete — `T::Item` once `T` is `SliceIter<i32>` —
/// replaced by what its implementation chose (spec §8.4, §8.5). Codegen never sees one.
fn table_body(
    table: &TypeTable,
    body: &Body,
    map: &HashMap<u32, Ty>,
    self_ty: Option<&Ty>,
) -> Body {
    dz_hir::map_body_types(body, |t| table.subst_with(t, map, self_ty))
}

/// How deeply type arguments nest: `f32` is 0, `Buffer<f32>` 1, `Buffer<Buffer<f32>>` 2.
///
/// Every type built from another counts a level, not only a generic one: `fn nest<T>(x: T)`
/// calling `nest([x])` instantiates at `[1]T`, `[1][1]T`, … and never names a generic type
/// at all. Counting only `Named` let that run until the compiler's own stack overflowed.
fn depth(t: &Ty) -> usize {
    match t {
        Ty::Named { args, .. } => 1 + args.iter().map(depth).max().unwrap_or(0),
        Ty::Optional(i) | Ty::Fallible(i) | Ty::Range(i) => 1 + depth(i),
        Ty::Ref { inner, .. } | Ty::Ptr { inner, .. } | Ty::OwnPtr { inner, .. } => {
            1 + depth(inner)
        }
        Ty::Array { elem, .. } | Ty::Slice { elem, .. } => 1 + depth(elem),
        Ty::Tuple(parts) => 1 + parts.iter().map(depth).max().unwrap_or(0),
        Ty::Fn { params, ret, .. } => {
            1 + params.iter().chain([&**ret]).map(depth).max().unwrap_or(0)
        }
        _ => 0,
    }
}
