//! The type checker (spec §5, §7, §9, §10, §11).
//!
//! M3's remit is a **non-generic** program. Signatures are collected for everything, then
//! every non-generic function body is checked.
//!
//! Deliberately deferred, each to the milestone that owns it:
//!
//! * generics and trait conformance — M6 (spec §8);
//! * ownership, moves and definite return — M5 (spec §6);
//! * macro bodies — M10; C++ imports — M7.
//!
//! Checking also **builds the HIR** (spec §17.1, `dz_hir`): every rule returns the type it
//! concluded together with the node it built for it. That is deliberate — the types this
//! pass computes are exactly the ones the HIR stores, and a second pass to collect them
//! would recompute them and could disagree. See `build.rs`.

use std::collections::HashMap;
use std::rc::Rc;

use dz_ast::*;
use dz_diag::{codes, Diagnostic, Diagnostics};
use dz_hir::Hir;
use dz_project::{ModuleId, Workspace};
use dz_resolve::{ParsedFile, Resolution, SymbolId, SymbolKind};
use dz_span::{FileId, Span};

mod build;
mod check;

/// The type model lives in `dz_hir`, since the HIR stores a `Ty` on every node and
/// `dz_hir` sits below this crate. Re-exported so `dz_check::ty::Ty` keeps working.
pub use dz_hir::ty;

use ty::{FloatTy, IntTy, Ty};

// ---------------------------------------------------------------------------
// signatures
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct FieldSig {
    pub name: String,
    pub ty: Ty,
    pub vis: Vis,
    pub has_default: bool,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct ParamSig {
    pub name: String,
    pub ty: Ty,
    pub has_default: bool,
}

#[derive(Clone, Debug)]
pub struct FnSig {
    pub params: Vec<ParamSig>,
    /// The **success** type. A fallible signature's failure is not part of it (spec §9).
    pub ret: Ty,
    /// `!->`: calling this may fail, so the call must be consumed by an `or` clause in the
    /// statement it appears in (spec §9.7).
    pub fallible: bool,
    /// `None` for a free function or a constructor.
    pub takes_self: Option<bool>,
    pub is_mut: bool,
    pub vis: Vis,
    pub span: Span,
    pub generic: bool,
}

impl FnSig {
    /// Parameters that must be supplied, i.e. those without a default (spec §7.2).
    pub fn required(&self) -> usize {
        self.params.iter().filter(|p| !p.has_default).count()
    }
}

#[derive(Clone, Debug, Default)]
pub struct StructSig {
    pub fields: Vec<FieldSig>,
    pub methods: HashMap<String, FnSig>,
    pub ctors: Vec<FnSig>,
    /// `copy(other: &Self)` — its presence is what makes an otherwise move-only type
    /// copyable (spec §6.3). The signature is kept for the parameter's span.
    pub copy: Option<FnSig>,
    /// `drop()` — its presence makes the type move-only and gives it a C++ destructor
    /// (spec §6.3, §6.6).
    pub drop: Option<FnSig>,
}

#[derive(Clone, Debug)]
pub struct VariantSig {
    pub name: String,
    pub fields: Vec<FieldSig>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum ItemSig {
    Struct(StructSig),
    Union {
        variants: Vec<VariantSig>,
    },
    Enum {
        backing: IntTy,
        variants: Vec<VariantSig>,
        /// The `= n` a variant was given, by position. `None` means "one more than the
        /// previous", which C++ spells the same way, so codegen prints nothing for it.
        values: Vec<Option<i128>>,
    },
    Trait(StructSig),
    Fn(FnSig),
    Const(Ty),
}

/// Everything the body checker needs to know about declarations.
pub struct TypeTable {
    sigs: HashMap<SymbolId, ItemSig>,
    /// Whether a declaration carries generic parameters, so bodies can be skipped.
    generic: HashMap<SymbolId, bool>,
}

impl TypeTable {
    pub fn get(&self, id: SymbolId) -> Option<&ItemSig> {
        self.sigs.get(&id)
    }

    pub fn struct_sig(&self, id: SymbolId) -> Option<&StructSig> {
        match self.sigs.get(&id)? {
            ItemSig::Struct(s) | ItemSig::Trait(s) => Some(s),
            _ => None,
        }
    }

    pub fn is_generic(&self, id: SymbolId) -> bool {
        self.generic.get(&id).copied().unwrap_or(false)
    }

    // --- ownership, the type-level half of spec §6 -------------------------
    //
    // Flow analysis asks these constantly and so does codegen, so they live next to the
    // signatures they read rather than being recomputed in each consumer. The
    // flow-sensitive half — *which* value is moved *where* — is `dz_flow`.

    /// Does a value of this type need work when it dies (spec §6.5)?
    ///
    /// True for a type with a `drop`, and for anything holding one, because C++ will
    /// generate the destructor that runs it. `own ^T` on its own does not: the pointer is
    /// released by the `drop` its struct is required to have (DZ0412), not by itself.
    pub fn needs_drop(&self, ty: &Ty) -> bool {
        match ty {
            Ty::Named(id) => match self.get(*id) {
                Some(ItemSig::Struct(s)) => {
                    s.drop.is_some() || s.fields.iter().any(|f| self.needs_drop(&f.ty))
                }
                Some(ItemSig::Union { variants }) => variants
                    .iter()
                    .any(|v| v.fields.iter().any(|f| self.needs_drop(&f.ty))),
                _ => false,
            },
            Ty::Array { elem, len: Some(_) } => self.needs_drop(elem),
            Ty::Tuple(parts) => parts.iter().any(|t| self.needs_drop(t)),
            _ => false,
        }
    }

    /// Spec §6.3. A type is copyable if it declares a `copy`, or if it declares neither
    /// `copy` nor `drop`, has no `own` field, and every field is copyable.
    ///
    /// Everything not named here is copyable: primitives, pointers, references and slices
    /// are all bit-copies, and a `^T` copies the pointer, not the pointee — `own ^T` is
    /// the spelling that says otherwise.
    pub fn copyable(&self, ty: &Ty) -> bool {
        self.copyable_guarded(ty, &mut Vec::new())
    }

    fn copyable_guarded(&self, ty: &Ty, stack: &mut Vec<SymbolId>) -> bool {
        match ty {
            // An `own ^T` *value* is one pointer, and reading it is a bit-copy like any
            // other. What `own` makes move-only is the **struct that declares it**, which
            // is the rule immediately below — spec §6.3 is careful to say "`own` on a
            // field". Treating the pointer itself as move-only would make `if handle !=
            // null` inside the type's own `drop` a diagnostic, which is absurd.
            Ty::Array { elem, .. } => self.copyable_guarded(elem, stack),
            Ty::Tuple(parts) => parts.iter().all(|t| self.copyable_guarded(t, stack)),
            Ty::Named(id) => {
                // A struct cannot contain itself by value, so a cycle here means the
                // program is already ill-formed; stop rather than recurse forever.
                if stack.contains(id) {
                    return true;
                }
                stack.push(*id);
                let answer = match self.get(*id) {
                    Some(ItemSig::Struct(s)) => {
                        if s.copy.is_some() {
                            true
                        } else if s.drop.is_some() {
                            false
                        } else {
                            // "no `own` field, and every field is copyable" (spec §6.3).
                            s.fields.iter().all(|f| {
                                !matches!(f.ty, Ty::OwnPtr(_))
                                    && self.copyable_guarded(&f.ty, stack)
                            })
                        }
                    }
                    Some(ItemSig::Union { variants }) => variants
                        .iter()
                        .all(|v| v.fields.iter().all(|f| self.copyable_guarded(&f.ty, stack))),
                    _ => true,
                };
                stack.pop();
                answer
            }
            _ => true,
        }
    }

    /// Why `ty` is move-only, phrased for the `help:` of a diagnostic. `None` when it is
    /// copyable after all.
    pub fn move_only_because(&self, ty: &Ty) -> Option<String> {
        if self.copyable(ty) {
            return None;
        }
        Some(match ty {
            Ty::Named(id) => match self.get(*id) {
                Some(ItemSig::Struct(s)) if s.drop.is_some() => {
                    "it declares a `drop`, so copying it would release the same resource \
                     twice"
                        .to_owned()
                }
                Some(ItemSig::Struct(s)) => {
                    match s.fields.iter().find(|f| matches!(f.ty, Ty::OwnPtr(_))) {
                        Some(f) => format!(
                            "its field `{}` is `own`, so the struct is responsible for                              what it points at",
                            f.name
                        ),
                        None => {
                            let field = s
                                .fields
                                .iter()
                                .find(|f| !self.copyable(&f.ty))
                                .map(|f| f.name.clone())
                                .unwrap_or_default();
                            format!("its field `{field}` is move-only")
                        }
                    }
                }
                _ => "it owns a resource".to_owned(),
            },
            _ => "it owns a resource".to_owned(),
        })
    }
}

pub struct CheckResult {
    pub table: TypeTable,
    /// Every checked body, resolved and typed (spec §17.1). Flow analysis (M5) and
    /// codegen consume this rather than the AST.
    pub hir: Hir,
    pub diagnostics: Diagnostics,
}

/// Check every module in the workspace.
pub fn check(
    ws: &Workspace,
    res: &Resolution,
    files: &HashMap<ModuleId, Vec<ParsedFile>>,
) -> CheckResult {
    let mut cx = Ctx {
        ws,
        res,
        diags: Diagnostics::new(),
        table: TypeTable {
            sigs: HashMap::new(),
            generic: HashMap::new(),
        },
        hir: Hir::default(),
        module: ModuleId(0),
        file: FileId(0),
    };

    // Pass 1: signatures, so bodies may refer to declarations in any order.
    for module in ws.modules() {
        cx.module = module.id;
        if let Some(module_files) = files.get(&module.id) {
            for pf in module_files {
                cx.file = pf.file;
                for item in &pf.ast.items {
                    cx.collect_item(item);
                }
            }
        }
    }

    // Pass 2: bodies.
    for module in ws.modules() {
        cx.module = module.id;
        if let Some(module_files) = files.get(&module.id) {
            for pf in module_files {
                cx.file = pf.file;
                for item in &pf.ast.items {
                    check::item(&mut cx, item);
                }
            }
        }
    }

    cx.diags.sort_by_position();
    CheckResult {
        table: cx.table,
        hir: cx.hir,
        diagnostics: cx.diags,
    }
}

pub(crate) struct Ctx<'a> {
    pub(crate) ws: &'a Workspace,
    pub(crate) res: &'a Resolution,
    pub(crate) diags: Diagnostics,
    pub(crate) table: TypeTable,
    pub(crate) hir: Hir,
    pub(crate) module: ModuleId,
    pub(crate) file: FileId,
}

impl<'a> Ctx<'a> {
    pub(crate) fn error(
        &mut self,
        code: &'static str,
        span: Span,
        msg: impl Into<String>,
        label: impl Into<String>,
    ) {
        self.diags
            .push(Diagnostic::error(code, msg.into()).at(self.file, span, label.into()));
    }

    pub(crate) fn error_help(
        &mut self,
        code: &'static str,
        span: Span,
        msg: impl Into<String>,
        label: impl Into<String>,
        help: impl Into<String>,
    ) {
        self.diags.push(
            Diagnostic::error(code, msg.into())
                .at(self.file, span, label.into())
                .with_help(help.into()),
        );
    }

    /// Render a type for a diagnostic.
    pub(crate) fn name_of(&self, t: &Ty) -> String {
        type_name(self.res, t)
    }
}

/// Symbol and type names for [`dz_hir::dump`], which stores ids and no text of its own.
pub struct Names<'a> {
    pub res: &'a Resolution,
}

impl dz_hir::dump::Names for Names<'_> {
    fn name_of_symbol(&self, id: SymbolId) -> String {
        self.res.symbol(id).name.clone()
    }

    fn name_of_type(&self, ty: &Ty) -> String {
        type_name(self.res, ty)
    }
}

/// Render a type as it is written in source.
pub fn type_name(res: &Resolution, t: &Ty) -> String {
    let name_of = |t: &Ty| type_name(res, t);
    match t {
        Ty::Error => "{unknown}".to_owned(),
        Ty::Never => "!".to_owned(),
        Ty::Void => "void".to_owned(),
        Ty::Bool => "bool".to_owned(),
        Ty::Char => "char".to_owned(),
        Ty::Int(i) => i.name().to_owned(),
        Ty::Float(f) => f.name().to_owned(),
        Ty::Str => "str".to_owned(),
        Ty::CStr => "cstr".to_owned(),
        Ty::IntLit => "{integer}".to_owned(),
        Ty::FloatLit => "{float}".to_owned(),
        Ty::Ptr(i) => format!("^{}", name_of(i)),
        Ty::OwnPtr(i) => format!("own ^{}", name_of(i)),
        Ty::Ref { mutable, inner } => {
            format!("&{}{}", if *mutable { "mut " } else { "" }, name_of(inner))
        }
        Ty::Optional(i) => format!("{}?", name_of(i)),
        // Not writable in source: the type of a `!->` call before its `or` (spec §9).
        Ty::Fallible(i) => format!("!-> {}", name_of(i)),
        Ty::Array { len: Some(n), elem } => format!("[{n}]{}", name_of(elem)),
        Ty::Array { len: None, elem } => format!("[]{}", name_of(elem)),
        Ty::Range(e) => format!("{}..{}", name_of(e), name_of(e)),
        Ty::Tuple(elems) => format!(
            "({})",
            elems.iter().map(&name_of).collect::<Vec<_>>().join(", ")
        ),
        Ty::Fn { params, ret } => format!(
            "fn({}) -> {}",
            params.iter().map(&name_of).collect::<Vec<_>>().join(", "),
            name_of(ret)
        ),
        Ty::Builtin(b) => b.name().to_owned(),
        Ty::Named(id) => res.symbol(*id).name.clone(),
        Ty::Variant(id, i) => {
            let owner = res.symbol(*id);
            let variant = owner.members.iter().filter(|m| m.sort == dz_resolve::MemberSort::Variant).nth(*i);
            match variant {
                Some(v) => format!("{}::{}", owner.name, v.name),
                None => owner.name.clone(),
            }
        }
        Ty::SelfTy => "Self".to_owned(),
    }
}

impl<'a> Ctx<'a> {
    // --- resolving syntax types into `Ty` ---------------------------------

    pub(crate) fn lower_type(&mut self, t: &Type) -> Ty {
        match &t.kind {
            TypeKind::Path(p) => self.lower_path_type(p, t.span),
            TypeKind::Ptr(i) => Ty::Ptr(Rc::new(self.lower_type(i))),
            TypeKind::OwnPtr(i) => Ty::OwnPtr(Rc::new(self.lower_type(i))),
            TypeKind::Ref { mutable, inner } => Ty::Ref {
                mutable: *mutable,
                inner: Rc::new(self.lower_type(inner)),
            },
            TypeKind::Optional(i) => Ty::Optional(Rc::new(self.lower_type(i))),
            TypeKind::Array { size, elem } => {
                let elem = Rc::new(self.lower_type(elem));
                let len = size.as_ref().and_then(|e| const_usize(e));
                if let Some(expr) = size {
                    if len.is_none() {
                        self.error_help(
                            codes::TYPE_MISMATCH,
                            expr.span,
                            "array length must be a constant",
                            "not a constant expression",
                            "use an integer literal or a `const` (spec §5.3)",
                        );
                    }
                }
                Ty::Array { len, elem }
            }
            TypeKind::Tuple(elems) => Ty::Tuple(elems.iter().map(|e| self.lower_type(e)).collect()),
            TypeKind::Fn { params, ret } => Ty::Fn {
                params: params.iter().map(|p| self.lower_type(p)).collect(),
                ret: Rc::new(ret.as_ref().map(|r| self.lower_type(r)).unwrap_or(Ty::Void)),
            },
            TypeKind::SelfType => Ty::SelfTy,
            TypeKind::Error => Ty::Error,
        }
    }

    fn lower_path_type(&mut self, p: &Path, span: Span) -> Ty {
        let Some(first) = p.segments.first() else {
            return Ty::Error;
        };
        let name = first.name.name.as_str();

        if p.segments.len() == 1 {
            if let Some(builtin) = builtin_type(name) {
                return builtin;
            }
            if let Some(sym) = self.res.lookup_in_file(self.module, self.file, name) {
                return self.named_or_error(sym, span);
            }
            // Name resolution already reported anything genuinely missing; an unknown
            // name here is a generic parameter, which M6 will type properly.
            return Ty::Error;
        }

        // `module::Type`
        if let Some(target) = self.res.module_alias(self.file, name) {
            if let Some(second) = p.segments.get(1) {
                if let Some(sym) = self.res.lookup(target, &second.name.name) {
                    return self.named_or_error(sym, span);
                }
            }
        }
        Ty::Error
    }

    fn named_or_error(&mut self, sym: SymbolId, span: Span) -> Ty {
        match self.res.symbol(sym).kind {
            SymbolKind::Struct | SymbolKind::Union | SymbolKind::Enum | SymbolKind::Trait => {
                Ty::Named(sym)
            }
            other => {
                let name = self.res.symbol(sym).name.clone();
                self.error(
                    codes::TYPE_MISMATCH,
                    span,
                    format!(
                        "`{name}` is a {} and cannot be used as a type",
                        other.describe()
                    ),
                    "not a type",
                );
                Ty::Error
            }
        }
    }

    // --- signature collection ---------------------------------------------

    fn collect_item(&mut self, item: &Item) {
        let Some(sym) = self.symbol_of(item) else {
            return;
        };
        match &item.kind {
            ItemKind::Struct(s) => {
                let generic = !s.generics.is_empty();
                self.table.generic.insert(sym, generic);
                let sig = self.collect_struct(&s.members, &s.name.name);
                self.table.sigs.insert(sym, ItemSig::Struct(sig));
            }
            ItemKind::Trait(t) => {
                self.table.generic.insert(sym, true); // trait bodies are M6
                let sig = self.collect_struct(&t.items, &t.name.name);
                self.table.sigs.insert(sym, ItemSig::Trait(sig));
            }
            ItemKind::Union(u) => {
                self.table.generic.insert(sym, !u.generics.is_empty());
                let variants = u
                    .variants
                    .iter()
                    .map(|v| VariantSig {
                        name: v.name.name.clone(),
                        fields: v.fields.iter().map(|f| self.field_sig(f)).collect(),
                        span: v.name.span,
                    })
                    .collect();
                self.table.sigs.insert(sym, ItemSig::Union { variants });
            }
            ItemKind::Enum(e) => {
                let backing = match &e.backing {
                    Some(t) => match self.lower_type(t) {
                        Ty::Int(i) => i,
                        other => {
                            if !other.is_error() {
                                let shown = self.name_of(&other);
                                self.error_help(
                                    codes::TYPE_MISMATCH,
                                    t.span,
                                    format!("`{shown}` is not a valid enum backing type"),
                                    "must be an integer type",
                                    "enums are named integers (spec §7.4)",
                                );
                            }
                            IntTy::I32
                        }
                    },
                    None => IntTy::I32,
                };
                let variants = e
                    .variants
                    .iter()
                    .map(|v| VariantSig {
                        name: v.name.name.clone(),
                        fields: Vec::new(),
                        span: v.name.span,
                    })
                    .collect();
                let values = e
                    .variants
                    .iter()
                    .map(|v| match v.value.as_ref().map(|x| &x.kind) {
                        Some(ExprKind::Int(text)) => parse_int_literal(text),
                        _ => None,
                    })
                    .collect();
                self.table
                    .sigs
                    .insert(sym, ItemSig::Enum { backing, variants, values });
            }
            ItemKind::Fn(f) => {
                let generic = !f.generics.is_empty();
                self.table.generic.insert(sym, generic);
                let sig = self.fn_sig(f, item.vis, generic);
                self.table.sigs.insert(sym, ItemSig::Fn(sig));
            }
            ItemKind::Const(c) => {
                let ty = match &c.ty {
                    Some(t) => self.lower_type(t),
                    // Without an annotation the value decides. Signature collection runs
                    // before any body, so only literals can be typed here; anything more
                    // involved needs the constant folding that arrives with M6.
                    None => literal_type(&c.value),
                };
                self.table.sigs.insert(sym, ItemSig::Const(ty));
            }
            ItemKind::Macro(_) | ItemKind::MacroInvocation { .. } | ItemKind::Error => {}
        }
    }

    fn collect_struct(&mut self, members: &[Member], _owner: &str) -> StructSig {
        let mut sig = StructSig::default();
        for m in members {
            match &m.kind {
                MemberKind::Field(f) => {
                    let mut field = self.field_sig(f);
                    // Visibility lives on the member, not the field node.
                    field.vis = m.vis;
                    // A reference cannot be stored in a field (spec §5.2, §18.3). This one
                    // rule is what lets Dazzle skip lifetime analysis entirely.
                    if let Ty::Ref { .. } = field.ty {
                        let shown = self.name_of(&field.ty);
                        self.error_help(
                            codes::REF_IN_FIELD,
                            f.ty.span,
                            format!("a struct field cannot hold `{shown}`"),
                            "references live only on the stack",
                            "use `^T` for a stored pointer — a reference in a field could \
                             outlive what it points at, and Dazzle has no lifetimes to \
                             catch that (spec §5.2)",
                        );
                    }
                    if sig.fields.iter().any(|x| x.name == field.name) {
                        let name = field.name.clone();
                        self.error(
                            codes::DUPLICATE_FIELD,
                            f.name.span,
                            format!("field `{name}` is declared twice"),
                            "duplicate field",
                        );
                    }
                    sig.fields.push(field);
                }
                MemberKind::Fn(f) => {
                    let generic = !f.generics.is_empty();
                    let s = self.fn_sig(f, m.vis, generic);
                    sig.methods.insert(f.name.name.clone(), s);
                }
                MemberKind::Ctor(f) => {
                    let s = self.fn_sig(f, m.vis, false);
                    sig.ctors.push(s);
                }
                // §6.1: at most one of each, and both are what §6.3 keys copyability on.
                MemberKind::Copy(f) => {
                    let s = self.fn_sig(f, m.vis, false);
                    if sig.copy.is_some() {
                        self.duplicate_special(f.name.span, "copy");
                    } else {
                        sig.copy = Some(s);
                    }
                }
                MemberKind::Drop(f) => {
                    let s = self.fn_sig(f, m.vis, false);
                    if sig.drop.is_some() {
                        self.duplicate_special(f.name.span, "drop");
                    } else {
                        sig.drop = Some(s);
                    }
                }
                MemberKind::TypeAlias { .. } | MemberKind::Error => {}
            }
        }
        sig
    }

    fn duplicate_special(&mut self, span: Span, what: &str) {
        self.error_help(
            codes::DUPLICATE_SPECIAL_MEMBER,
            span,
            format!("`{what}` is declared twice"),
            format!("a struct may declare at most one `{what}`"),
            "the three special members are a constructor, `copy` and `drop`, and there is \
             one of each (spec §6.1)",
        );
    }

    fn field_sig(&mut self, f: &Field) -> FieldSig {
        let mut ty = self.lower_type(&f.ty);
        if f.is_own {
            // `own ^T` is written as a pointer; the parser records `own` separately when
            // it appears as a field modifier.
            if let Ty::Ptr(inner) = ty {
                ty = Ty::OwnPtr(inner);
            }
        }
        FieldSig {
            name: f.name.name.clone(),
            ty,
            vis: Vis::Private,
            has_default: f.default.is_some(),
            span: f.name.span,
        }
    }

    fn fn_sig(&mut self, f: &FnDecl, vis: Vis, generic: bool) -> FnSig {
        let params = f
            .params
            .iter()
            .map(|p| ParamSig {
                name: p.name.name.clone(),
                ty: self.lower_type(&p.ty),
                has_default: p.default.is_some(),
            })
            .collect();
        let ret = f
            .ret
            .as_ref()
            .map(|r| self.lower_type(r))
            .unwrap_or(Ty::Void);
        FnSig {
            params,
            ret,
            fallible: f.fallible,
            takes_self: f.self_param.map(|s| s.is_mut),
            is_mut: f.is_mut,
            vis,
            span: f.name.span,
            generic,
        }
    }

    pub(crate) fn symbol_of(&self, item: &Item) -> Option<SymbolId> {
        let name = match &item.kind {
            ItemKind::Fn(f) => &f.name.name,
            ItemKind::Struct(s) => &s.name.name,
            ItemKind::Union(u) => &u.name.name,
            ItemKind::Enum(e) => &e.name.name,
            ItemKind::Trait(t) => &t.name.name,
            ItemKind::Const(c) => &c.name.name,
            ItemKind::Macro(m) => &m.name.name,
            ItemKind::MacroInvocation { .. } | ItemKind::Error => return None,
        };
        self.res.lookup(self.module, name)
    }
}

/// Field visibility comes from the member, not the field node, so it is patched in after
/// signature collection walks the members.
pub(crate) fn builtin_type(name: &str) -> Option<Ty> {
    if let Some(i) = IntTy::from_name(name) {
        return Some(Ty::Int(i));
    }
    Some(match name {
        "f32" => Ty::Float(FloatTy::F32),
        "f64" => Ty::Float(FloatTy::F64),
        "bool" => Ty::Bool,
        "char" => Ty::Char,
        "str" => Ty::Str,
        "cstr" => Ty::CStr,
        "void" => Ty::Void,
        "Self" => Ty::SelfTy,
        // `Error` and `SourceLocation` are provided by the language, not the stdlib
        // (spec §9.2, §12.7), so they are spellable without an import.
        other => match ty::BuiltinTy::from_name(other) {
            Some(b) => Ty::Builtin(b),
            None => return None,
        },
    })
}

/// Evaluate a constant array length. Only literals for now; `const` folding is M6.
fn const_usize(e: &Expr) -> Option<u64> {
    match &e.kind {
        ExprKind::Int(text) => parse_int_literal(text).and_then(|v| u64::try_from(v).ok()),
        _ => None,
    }
}

/// Parse an integer literal's numeric value, ignoring separators and any type suffix.
pub(crate) fn parse_int_literal(text: &str) -> Option<i128> {
    let cleaned: String = text.chars().filter(|c| *c != '_').collect();
    let (digits, radix) =
        if let Some(rest) = cleaned.strip_prefix("0x").or(cleaned.strip_prefix("0X")) {
            (rest, 16)
        } else if let Some(rest) = cleaned.strip_prefix("0o").or(cleaned.strip_prefix("0O")) {
            (rest, 8)
        } else if let Some(rest) = cleaned.strip_prefix("0b").or(cleaned.strip_prefix("0B")) {
            (rest, 2)
        } else {
            (cleaned.as_str(), 10)
        };
    // Strip a trailing type suffix such as `u32`.
    let end = digits
        .char_indices()
        .find(|(_, c)| !c.is_digit(radix) && *c != '_')
        .map(|(i, _)| i)
        .unwrap_or(digits.len());
    i128::from_str_radix(&digits[..end], radix).ok()
}

/// Decode a string or character literal into the bytes it denotes: strip the quotes and
/// apply the escapes of spec §2.4.
///
/// The AST keeps literals as the exact source text, because the formatter has to reproduce
/// what was written. The HIR keeps the value, because everything downstream wants the
/// bytes. The lexer has already rejected malformed escapes, so anything unrecognised here
/// is passed through rather than reported a second time.
pub(crate) fn decode_string_literal(text: &str) -> String {
    let body = text
        .strip_prefix('"')
        .and_then(|t| t.strip_suffix('"'))
        .unwrap_or(text);
    let body = body
        .strip_prefix('\'')
        .and_then(|t| t.strip_suffix('\''))
        .unwrap_or(body);
    let mut out = String::with_capacity(body.len());
    let mut chars = body.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('0') => out.push('\0'),
            Some('\\') => out.push('\\'),
            Some('"') => out.push('"'),
            Some('\'') => out.push('\''),
            Some('x') => {
                let hex: String = chars.by_ref().take(2).collect();
                match u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                    Some(c) => out.push(c),
                    None => out.push('\u{FFFD}'),
                }
            }
            Some('u') => {
                let mut hex = String::new();
                for h in chars.by_ref() {
                    match h {
                        '{' => {}
                        '}' => break,
                        _ => hex.push(h),
                    }
                }
                match u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                    Some(c) => out.push(c),
                    None => out.push('\u{FFFD}'),
                }
            }
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

/// Strip the delimiters of a raw string literal, which has no escapes (spec §2.4).
pub(crate) fn decode_raw_string_literal(text: &str) -> String {
    text.strip_prefix('"')
        .and_then(|t| t.strip_suffix('"'))
        .unwrap_or(text)
        .to_owned()
}

/// Parse a float literal's value, ignoring separators and any type suffix.
pub(crate) fn parse_float_literal(text: &str) -> f64 {
    let cleaned: String = text.chars().filter(|c| *c != '_').collect();
    let end = cleaned
        .char_indices()
        .find(|(_, c)| !c.is_ascii_digit() && !matches!(c, '.' | 'e' | 'E' | '+' | '-'))
        .map(|(i, _)| i)
        .unwrap_or(cleaned.len());
    cleaned[..end].parse().unwrap_or(0.0)
}

/// The type of a literal, for a `const` with no annotation. Untyped literals take their
/// default here because a `const` has no context to fix them (spec §2.5).
fn literal_type(e: &Expr) -> Ty {
    match &e.kind {
        ExprKind::Int(_) => Ty::Int(IntTy::I32),
        ExprKind::Float(_) => Ty::Float(FloatTy::F64),
        ExprKind::Str(_) | ExprKind::RawStr(_) => Ty::Str,
        ExprKind::Char(_) => Ty::Char,
        ExprKind::Bool(_) => Ty::Bool,
        ExprKind::Unary {
            op: UnaryOp::Neg,
            operand,
        } => literal_type(operand),
        _ => Ty::Error,
    }
}
