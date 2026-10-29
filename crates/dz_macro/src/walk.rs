//! A walk over every node of a tree that can hold a name or an invocation, with hooks.
//!
//! Two passes need the whole tree: expansion, which finds invocations in bodies and
//! replaces them, and hygiene, which renames what an expansion introduced. Each overrides
//! only the hooks it cares about; the walk itself is written once, so a construct added to
//! the AST is added here and both passes see it.

use dz_ast::*;

pub(crate) trait Hooks {
    /// A local's name where it is bound: `let`, a parameter, a `for` or `case` binding, an
    /// `or { e -> … }` name, an `if let`.
    fn binder(&mut self, _id: &mut Ident) {}
    /// A path in expression position, `x` or `engine::Thing`.
    fn value_path(&mut self, _p: &mut Path) {}
    /// A path in type position.
    fn type_path(&mut self, _p: &mut Path) {}
    /// The parameter a function's tie path starts from: `world` in `kills world.enemies@items`
    /// (spec §6.7). Renamed with the parameter, so the clause still names it.
    fn tie_root(&mut self, _id: &mut Ident) {}
    /// A block's statements, before they are walked — where statement invocations are
    /// replaced.
    fn stmts(&mut self, _stmts: &mut Vec<Stmt>) {}
    /// An expression, before its parts are walked — where expression invocations are
    /// replaced.
    fn expr(&mut self, _e: &mut Expr) {}
    /// A struct literal's `Foo{ x }`, whose field is named by the expression it holds.
    fn field_init(&mut self, _f: &mut FieldInit) {}
    /// A block, after everything in it has been walked.
    fn block_done(&mut self, _b: &mut Block) {}
    /// A statement, after everything in it has been walked.
    fn stmt_done(&mut self, _s: &mut Stmt) {}
}

pub(crate) fn items<H: Hooks>(h: &mut H, items: &mut [Item]) {
    for i in items {
        item(h, i);
    }
}

pub(crate) fn item<H: Hooks>(h: &mut H, item: &mut Item) {
    match &mut item.kind {
        ItemKind::Fn(f) => fn_decl(h, f),
        ItemKind::Struct(s) => {
            generics(h, &mut s.generics);
            if let Some(b) = &mut s.base {
                h.type_path(b);
            }
            members(h, &mut s.members);
        }
        ItemKind::Union(u) => {
            generics(h, &mut u.generics);
            for v in &mut u.variants {
                for f in &mut v.fields {
                    field(h, f);
                }
            }
            members(h, &mut u.methods);
        }
        ItemKind::Enum(e) => {
            if let Some(t) = &mut e.backing {
                ty(h, t);
            }
            for v in &mut e.variants {
                if let Some(x) = &mut v.value {
                    expr(h, x);
                }
            }
        }
        ItemKind::Trait(t) => {
            generics(h, &mut t.generics);
            for p in &mut t.supertraits {
                h.type_path(p);
            }
            members(h, &mut t.items);
        }
        ItemKind::Impl(i) => {
            generics(h, &mut i.generics);
            if let Some(p) = &mut i.trait_path {
                h.type_path(p);
            }
            ty(h, &mut i.self_ty);
            members(h, &mut i.items);
        }
        ItemKind::Const(c) => {
            if let Some(t) = &mut c.ty {
                ty(h, t);
            }
            expr(h, &mut c.value);
        }
        ItemKind::Static(s) => {
            ty(h, &mut s.ty);
            expr(h, &mut s.value);
        }
        ItemKind::CfgIf(c) => cfg_items(h, c),
        ItemKind::Macro(_) | ItemKind::MacroInvocation(_) | ItemKind::Error => {}
    }
}

fn cfg_items<H: Hooks>(h: &mut H, c: &mut CfgIf<Item>) {
    items(h, &mut c.then);
    match &mut c.otherwise {
        Some(CfgElse::If(n)) => cfg_items(h, n),
        Some(CfgElse::Else { items: xs, .. }) => items(h, xs),
        None => {}
    }
}

fn generics<H: Hooks>(h: &mut H, g: &mut Generics) {
    for p in &mut g.params {
        for b in &mut p.bounds {
            h.type_path(b);
        }
        if let Some(d) = &mut p.default {
            ty(h, d);
        }
    }
}

fn members<H: Hooks>(h: &mut H, ms: &mut [Member]) {
    for m in ms {
        member(h, m);
    }
}

fn member<H: Hooks>(h: &mut H, m: &mut Member) {
    match &mut m.kind {
        MemberKind::Field(f) => field(h, f),
        MemberKind::Fn(f) | MemberKind::Ctor(f) => fn_decl(h, f),
        MemberKind::TypeAlias { bounds, ty: t, .. } => {
            for b in bounds {
                h.type_path(b);
            }
            if let Some(t) = t {
                ty(h, t);
            }
        }
        MemberKind::CfgIf(c) => cfg_members(h, c),
        MemberKind::Const(c) => {
            if let Some(t) = &mut c.ty {
                ty(h, t);
            }
            expr(h, &mut c.value);
        }
        // A tie's paths start at fields, which are not binders.
        MemberKind::Tie(_) | MemberKind::Error => {}
    }
}

fn cfg_members<H: Hooks>(h: &mut H, c: &mut CfgIf<Member>) {
    members(h, &mut c.then);
    match &mut c.otherwise {
        Some(CfgElse::If(n)) => cfg_members(h, n),
        Some(CfgElse::Else { items: xs, .. }) => members(h, xs),
        None => {}
    }
}

fn field<H: Hooks>(h: &mut H, f: &mut Field) {
    ty(h, &mut f.ty);
    if let Some(d) = &mut f.default {
        expr(h, d);
    }
}

fn fn_decl<H: Hooks>(h: &mut H, f: &mut FnDecl) {
    generics(h, &mut f.generics);
    for p in &mut f.params {
        h.binder(&mut p.name);
        ty(h, &mut p.ty);
        if let Some(d) = &mut p.default {
            expr(h, d);
        }
    }
    if let Some(r) = &mut f.ret {
        ty(h, r);
    }
    // A function type's clause names that type's own parameters, which are not binders, so
    // only a declaration's tie paths are walked.
    let ties = f.params.iter_mut().filter_map(|p| p.tie.as_mut());
    for t in ties.chain(f.ret_tie.as_mut()).chain(f.kills.iter_mut()) {
        h.tie_root(&mut t.root);
    }
    if let Some(b) = &mut f.base_init {
        h.type_path(&mut b.path);
        for a in &mut b.args {
            expr(h, a);
        }
    }
    if let Some(body) = &mut f.body {
        block(h, body);
    }
}

pub(crate) fn ty<H: Hooks>(h: &mut H, t: &mut Type) {
    match &mut t.kind {
        TypeKind::Path(p) => {
            h.type_path(p);
            for s in &mut p.segments {
                for g in &mut s.generics {
                    ty(h, g);
                }
            }
        }
        TypeKind::Ptr { inner, .. }
        | TypeKind::OwnPtr { inner, .. }
        | TypeKind::Ref { inner, .. }
        | TypeKind::Optional(inner) => ty(h, inner),
        TypeKind::Array { size, elem } => {
            expr(h, size);
            ty(h, elem);
        }
        TypeKind::Slice { elem, .. } => ty(h, elem),
        TypeKind::Tuple(ts) => {
            for t in ts {
                ty(h, t);
            }
        }
        TypeKind::Fn { params, ret, .. } => {
            for p in params {
                ty(h, p);
            }
            if let Some(r) = ret {
                ty(h, r);
            }
        }
        TypeKind::Const(inner) => ty(h, inner),
        TypeKind::SelfType | TypeKind::Error => {}
    }
}

pub(crate) fn block<H: Hooks>(h: &mut H, b: &mut Block) {
    stmts(h, &mut b.stmts);
    h.block_done(b);
}

pub(crate) fn stmts<H: Hooks>(h: &mut H, ss: &mut Vec<Stmt>) {
    h.stmts(ss);
    for s in ss {
        stmt(h, s);
    }
}

fn pat<H: Hooks>(h: &mut H, p: &mut Pat) {
    match &mut p.kind {
        PatKind::Ident(id) => h.binder(id),
        PatKind::Tuple(ps) => {
            for p in ps {
                pat(h, p);
            }
        }
        PatKind::Wild | PatKind::Error => {}
    }
}

fn stmt<H: Hooks>(h: &mut H, s: &mut Stmt) {
    match &mut s.kind {
        StmtKind::Let(l) => {
            // The initialiser first: in `let x = x` the right-hand `x` is the outer one.
            if let Some(t) = &mut l.ty {
                ty(h, t);
            }
            if let Some(i) = &mut l.init {
                expr(h, i);
            }
            pat(h, &mut l.pat);
        }
        StmtKind::Assign { target, value, .. } => {
            expr(h, target);
            expr(h, value);
        }
        StmtKind::Expr(e) => expr(h, e),
        StmtKind::While(w) => {
            expr(h, &mut w.cond);
            block(h, &mut w.body);
        }
        StmtKind::For(f) => {
            expr(h, &mut f.iter);
            for b in &mut f.bindings {
                pat(h, &mut b.pat);
            }
            block(h, &mut f.body);
        }
        StmtKind::Defer(inner) => stmt(h, inner),
        StmtKind::Item(i) => item(h, i),
        StmtKind::CfgIf(c) => cfg_stmts(h, c),
        StmtKind::Macro(_) | StmtKind::Error => {}
    }
    h.stmt_done(s);
}

fn cfg_stmts<H: Hooks>(h: &mut H, c: &mut CfgIf<Stmt>) {
    stmts(h, &mut c.then);
    match &mut c.otherwise {
        Some(CfgElse::If(n)) => cfg_stmts(h, n),
        Some(CfgElse::Else { items: xs, .. }) => stmts(h, xs),
        None => {}
    }
}

pub(crate) fn expr<H: Hooks>(h: &mut H, e: &mut Expr) {
    h.expr(e);
    match &mut e.kind {
        ExprKind::Path(p) => {
            h.value_path(p);
            for s in &mut p.segments {
                for g in &mut s.generics {
                    ty(h, g);
                }
            }
        }
        ExprKind::DotInfer { init, .. } => {
            for f in init.iter_mut().flatten() {
                h.field_init(f);
                expr(h, &mut f.value);
            }
        }
        ExprKind::StructLit { path, fields } => {
            h.type_path(path);
            for f in fields {
                h.field_init(f);
                expr(h, &mut f.value);
            }
        }
        ExprKind::ArrayLit(xs) | ExprKind::Tuple(xs) => {
            for x in xs {
                expr(h, x);
            }
        }
        ExprKind::Call { callee, args } => {
            expr(h, callee);
            for a in args {
                expr(h, a);
            }
        }
        ExprKind::Index { base, index } => {
            expr(h, base);
            expr(h, index);
        }
        ExprKind::Field { base, generics, .. } => {
            expr(h, base);
            for g in generics {
                ty(h, g);
            }
        }
        ExprKind::Unary { operand, .. } => expr(h, operand),
        // `allocator = a`: the name is the parameter's, not a binding or a path.
        ExprKind::Named { value, .. } => expr(h, value),
        ExprKind::Binary { lhs, rhs, .. } => {
            expr(h, lhs);
            expr(h, rhs);
        }
        ExprKind::Cast { expr: x, ty: t } => {
            expr(h, x);
            ty(h, t);
        }
        ExprKind::Range { start, end, .. } => {
            for x in [start, end].into_iter().flatten() {
                expr(h, x);
            }
        }
        ExprKind::Ref { expr: x, .. } | ExprKind::Deref(x) | ExprKind::Move(x) => expr(h, x),
        ExprKind::Or { expr: x, clause } => {
            expr(h, x);
            match clause.as_mut() {
                OrClause::Value(v) => expr(h, v),
                OrClause::Block { binding, body } => {
                    if let Some(b) = binding {
                        h.binder(b);
                    }
                    block(h, body);
                }
                OrClause::Ignore(_) | OrClause::Unreachable(_) => {}
            }
        }
        ExprKind::Block(b) | ExprKind::Unsafe(b) => block(h, b),
        ExprKind::If(i) => {
            expr(h, &mut i.cond);
            if let Some(b) = &mut i.binding {
                h.binder(&mut b.name);
            }
            block(h, &mut i.then);
            if let Some(x) = &mut i.else_branch {
                expr(h, x);
            }
        }
        ExprKind::Switch(s) => {
            expr(h, &mut s.subject);
            for arm in &mut s.arms {
                for p in &mut arm.patterns {
                    match p {
                        CasePat::Variant(v) => {
                            if let Some(b) = &mut v.binding {
                                h.binder(b);
                            }
                        }
                        CasePat::Value(v) => expr(h, v),
                    }
                }
                block(h, &mut arm.body);
            }
            if let Some(d) = &mut s.default {
                block(h, d);
            }
        }
        ExprKind::Return(Some(v)) => expr(h, v),
        ExprKind::Intrinsic { args, .. } => {
            for a in args {
                expr(h, a);
            }
        }
        ExprKind::Int(_)
        | ExprKind::Float(_)
        | ExprKind::Str(_)
        | ExprKind::RawStr(_)
        | ExprKind::Char(_)
        | ExprKind::Bool(_)
        | ExprKind::Null
        | ExprKind::None
        | ExprKind::Return(None)
        | ExprKind::Break { .. }
        | ExprKind::Continue { .. }
        | ExprKind::MacroCall(_)
        | ExprKind::Error => {}
    }
}
