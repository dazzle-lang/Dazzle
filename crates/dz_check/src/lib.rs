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
//! Types are computed and discarded rather than stored in a side table. When codegen needs
//! them (M4 onwards) the right move is the HIR that spec §17.1 already calls for, not a
//! span-keyed map bolted onto the AST.

use std::collections::HashMap;
use std::rc::Rc;

use dz_ast::*;
use dz_diag::{codes, Diagnostic, Diagnostics};
use dz_project::{ModuleId, Workspace};
use dz_resolve::{ParsedFile, Resolution, SymbolId, SymbolKind};
use dz_span::{FileId, Span};

mod check;
pub mod ty;

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
    pub ret: Ty,
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
    Union { variants: Vec<VariantSig> },
    Enum { backing: IntTy, variants: Vec<VariantSig> },
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
}

pub struct CheckResult {
    pub table: TypeTable,
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
        table: TypeTable { sigs: HashMap::new(), generic: HashMap::new() },
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
    CheckResult { table: cx.table, diagnostics: cx.diags }
}

pub(crate) struct Ctx<'a> {
    pub(crate) ws: &'a Workspace,
    pub(crate) res: &'a Resolution,
    pub(crate) diags: Diagnostics,
    pub(crate) table: TypeTable,
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
        self.diags.push(
            Diagnostic::error(code, msg.into()).at(self.file, span, label.into()),
        );
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
            Ty::Ptr(i) => format!("^{}", self.name_of(i)),
            Ty::OwnPtr(i) => format!("own ^{}", self.name_of(i)),
            Ty::Ref { mutable, inner } => {
                format!("&{}{}", if *mutable { "mut " } else { "" }, self.name_of(inner))
            }
            Ty::Optional(i) => format!("?{}", self.name_of(i)),
            Ty::Fallible(i) => format!("!{}", self.name_of(i)),
            Ty::Array { len: Some(n), elem } => format!("[{n}]{}", self.name_of(elem)),
            Ty::Array { len: None, elem } => format!("[]{}", self.name_of(elem)),
            Ty::Range(e) => format!("{}..{}", self.name_of(e), self.name_of(e)),
            Ty::Tuple(elems) => format!(
                "({})",
                elems.iter().map(|e| self.name_of(e)).collect::<Vec<_>>().join(", ")
            ),
            Ty::Fn { params, ret } => format!(
                "fn({}) -> {}",
                params.iter().map(|p| self.name_of(p)).collect::<Vec<_>>().join(", "),
                self.name_of(ret)
            ),
            Ty::Named(id) => self.res.symbol(*id).name.clone(),
            Ty::SelfTy => "Self".to_owned(),
        }
    }

    // --- resolving syntax types into `Ty` ---------------------------------

    pub(crate) fn lower_type(&mut self, t: &Type) -> Ty {
        match &t.kind {
            TypeKind::Path(p) => self.lower_path_type(p, t.span),
            TypeKind::Ptr(i) => Ty::Ptr(Rc::new(self.lower_type(i))),
            TypeKind::OwnPtr(i) => Ty::OwnPtr(Rc::new(self.lower_type(i))),
            TypeKind::Ref { mutable, inner } => {
                Ty::Ref { mutable: *mutable, inner: Rc::new(self.lower_type(inner)) }
            }
            TypeKind::Optional(i) => Ty::Optional(Rc::new(self.lower_type(i))),
            TypeKind::Fallible(i) => Ty::Fallible(Rc::new(self.lower_type(i))),
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
            TypeKind::Tuple(elems) => {
                Ty::Tuple(elems.iter().map(|e| self.lower_type(e)).collect())
            }
            TypeKind::Fn { params, ret } => Ty::Fn {
                params: params.iter().map(|p| self.lower_type(p)).collect(),
                ret: Rc::new(ret.as_ref().map(|r| self.lower_type(r)).unwrap_or(Ty::Void)),
            },
            TypeKind::SelfType => Ty::SelfTy,
            TypeKind::Error => Ty::Error,
        }
    }

    fn lower_path_type(&mut self, p: &Path, span: Span) -> Ty {
        let Some(first) = p.segments.first() else { return Ty::Error };
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
                    format!("`{name}` is a {} and cannot be used as a type", other.describe()),
                    "not a type",
                );
                Ty::Error
            }
        }
    }

    // --- signature collection ---------------------------------------------

    fn collect_item(&mut self, item: &Item) {
        let Some(sym) = self.symbol_of(item) else { return };
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
                self.table.sigs.insert(sym, ItemSig::Enum { backing, variants });
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
                MemberKind::Copy(_) | MemberKind::Drop(_) => {}
                MemberKind::TypeAlias { .. } | MemberKind::Error => {}
            }
        }
        sig
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
        let ret = f.ret.as_ref().map(|r| self.lower_type(r)).unwrap_or(Ty::Void);
        FnSig {
            params,
            ret,
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
        _ => return None,
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
    let (digits, radix) = if let Some(rest) = cleaned.strip_prefix("0x").or(cleaned.strip_prefix("0X")) {
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

/// The type of a literal, for a `const` with no annotation. Untyped literals take their
/// default here because a `const` has no context to fix them (spec §2.5).
fn literal_type(e: &Expr) -> Ty {
    match &e.kind {
        ExprKind::Int(_) => Ty::Int(IntTy::I32),
        ExprKind::Float(_) => Ty::Float(FloatTy::F64),
        ExprKind::Str(_) | ExprKind::RawStr(_) => Ty::Str,
        ExprKind::Char(_) => Ty::Char,
        ExprKind::Bool(_) => Ty::Bool,
        ExprKind::Unary { op: UnaryOp::Neg, operand } => literal_type(operand),
        _ => Ty::Error,
    }
}
