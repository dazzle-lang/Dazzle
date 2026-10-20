//! Reflection's type table (spec §13).
//!
//! Every emitter interns the types the program reflects on as it meets them — `typeinfo_of`
//! and `typeid_of` name one — and the table is written last, into
//! `dz_typeinfo.cpp`, once the closure over fields and element types is known. Ids are
//! handed out in the order types are met, so an id written into one module's C++ stays
//! right however many types are added after it.
//!
//! The table is filled in the first time it is read, by the constructor of a function-local
//! static: the generated structs have constructors, so it cannot be a constant aggregate,
//! and a global constructor would break §16.4. Offsets come from `offsetof`, so the C++
//! compiler is the one that knows the layout.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt::Write;

use dz_check::{ItemSig, TypeTable};
use dz_hir::ty::{FloatTy, IntTy, Ty};
use dz_resolve::{SymbolId, SymbolKind};

use super::{namespace_close, namespace_open, quote_cpp, sanitize, Emitter};

/// The types reflected on so far, by id.
#[derive(Default)]
pub(crate) struct Reflection {
    types: Vec<Ty>,
    /// Each type's C++ spelling, which names it uniquely, to its id.
    ids: HashMap<String, u32>,
    /// Something reads the table by id (`typeinfo(id)`), so it must exist even if empty.
    needed: bool,
}

impl Reflection {
    pub(crate) fn is_needed(&self) -> bool {
        self.needed || !self.types.is_empty()
    }
}

impl<'a> Emitter<'a> {
    /// The id of `ty` in the type table, adding it when it is new.
    pub(crate) fn reflect_id(&mut self, ty: &Ty) -> u32 {
        let key = self.table_spelling(ty);
        let mut r = self.reflect.borrow_mut();
        r.needed = true;
        if let Some(id) = r.ids.get(&key) {
            return *id;
        }
        let id = r.types.len() as u32;
        r.types.push(ty.clone());
        r.ids.insert(key, id);
        id
    }

    /// A type's C++ spelling as seen from the standard library's namespace, where the table
    /// is written — the same whichever module met the type, so it keys the type too.
    fn table_spelling(&mut self, ty: &Ty) -> String {
        let saved = self.module;
        self.module = self.res.prelude;
        let s = self.cpp_ty(ty);
        self.module = saved;
        s
    }

    fn std_cpp(&mut self, name: &str) -> String {
        match self.res.lookup(self.res.prelude, name) {
            Some(sym) => self.cpp_ty(&Ty::plain(sym)),
            None => "void".to_owned(),
        }
    }

    /// `typeinfo_of<T>()` and `typeid_of<T>()` (spec §13.1).
    pub(crate) fn reflect_expr(&mut self, ty: &Ty, info: bool) -> String {
        let id = self.reflect_id(ty);
        if info {
            let t = self.std_cpp("TypeInfo");
            format!("(*static_cast<const {t}*>(::dz::reflect_entry({id}u)))")
        } else {
            let t = self.std_cpp("TypeId");
            format!("([]() {{ {t} id_; id_.index = {id}u; return id_; }}())")
        }
    }

    /// `typeinfo(id)`, reading the table by the id's index.
    pub(crate) fn typeinfo_by_id(&mut self, id: &str) -> String {
        self.reflect.borrow_mut().needed = true;
        let t = self.std_cpp("TypeInfo");
        format!("(*static_cast<const {t}*>(::dz::reflect_entry(({id}).index)))")
    }
}

/// One `FieldInfo` of the table, before it is written.
struct FieldRow {
    name: String,
    ty: u32,
    /// C++ expression for the offset.
    offset: String,
    vis: &'static str,
    owning: bool,
    tag: String,
}

struct VariantRow {
    name: String,
    /// C++ expression for the value.
    value: String,
    fields: Vec<FieldRow>,
}

#[derive(Default)]
struct TypeRow {
    name: String,
    module: String,
    size: String,
    align: String,
    kind: &'static str,
    fields: Vec<FieldRow>,
    variants: Vec<VariantRow>,
    elem: u32,
    len: u64,
    mutable: bool,
    tag_offset: String,
}

/// The types `type_info = "all"` puts in the table whether or not anything names them
/// (spec §13.2): every non-generic struct, union and enum the program declares.
pub(crate) fn seed_all(
    e: &mut Emitter<'_>,
    files: &HashMap<dz_project::ModuleId, Vec<dz_resolve::ParsedFile>>,
) {
    let mut seeds: Vec<SymbolId> = Vec::new();
    for pfs in files.values() {
        for pf in pfs {
            for item in &pf.ast.items {
                let name = match &item.kind {
                    dz_ast::ItemKind::Struct(s) if s.generics.params.is_empty() => &s.name,
                    dz_ast::ItemKind::Union(u) if u.generics.params.is_empty() => &u.name,
                    dz_ast::ItemKind::Enum(en) => &en.name,
                    _ => continue,
                };
                let Some(module) = e.res.file_module.get(&pf.file) else { continue };
                if let Some(sym) = e.res.lookup(*module, &name.name) {
                    seeds.push(sym);
                }
            }
        }
    }
    seeds.sort();
    for sym in seeds {
        e.reflect_id(&Ty::plain(sym));
    }
}

/// `dz_typeinfo.cpp`: the table, and `::dz::reflect_entry`, which reads it.
pub(crate) fn table_file(e: &mut Emitter<'_>, headers: &[String]) -> String {
    // The closure: describing a type interns what it is made of, which may add more.
    let mut rows: Vec<TypeRow> = Vec::new();
    let mut i = 0;
    while i < e.reflect.borrow().types.len() {
        let ty = e.reflect.borrow().types[i].clone();
        let row = describe(e, &ty);
        rows.push(row);
        i += 1;
    }

    let info = e.std_cpp("TypeInfo");
    let field = e.std_cpp("FieldInfo");
    let variant = e.std_cpp("VariantInfo");
    let kind = e.std_cpp("TypeKind");
    let vis = e.std_cpp("Visibility");

    let mut strings: Vec<String> = Vec::new();
    let intern = |s: &str, strings: &mut Vec<String>| -> String {
        let n = strings.len();
        strings.push(s.to_owned());
        format!("::dz::str(dz_s{n}, {})", s.len())
    };

    let field_count: usize = rows
        .iter()
        .map(|r| r.fields.len() + r.variants.iter().map(|v| v.fields.len()).sum::<usize>())
        .sum();
    let variant_count: usize = rows.iter().map(|r| r.variants.len()).sum();

    let mut fill = String::new();
    let mut next_field = 0usize;
    let mut next_variant = 0usize;
    let write_fields = |fields: &[FieldRow],
                        fill: &mut String,
                        strings: &mut Vec<String>,
                        next_field: &mut usize|
     -> String {
        let start = *next_field;
        for f in fields {
            let n = *next_field;
            let name = intern(&f.name, strings);
            let tag = intern(&f.tag, strings);
            let _ = writeln!(
                fill,
                "        f[{n}].name = {name}; f[{n}].type_id.index = {}u; f[{n}].offset = {}; \
                 f[{n}].vis = {vis}::{}; f[{n}].owning = {}; f[{n}].tag = {tag};",
                f.ty, f.offset, f.vis, f.owning
            );
            *next_field += 1;
        }
        if fields.is_empty() {
            format!("::dz::Slice<const {field}>()")
        } else {
            format!("::dz::Slice<const {field}>(&f[{start}], {})", fields.len())
        }
    };
    for (id, r) in rows.iter().enumerate() {
        let name = intern(&r.name, &mut strings);
        let module = intern(&r.module, &mut strings);
        let fields = write_fields(&r.fields, &mut fill, &mut strings, &mut next_field);
        let variants = if r.variants.is_empty() {
            format!("::dz::Slice<const {variant}>()")
        } else {
            let start = next_variant;
            for v in &r.variants {
                let n = next_variant;
                let vname = intern(&v.name, &mut strings);
                let vfields = write_fields(&v.fields, &mut fill, &mut strings, &mut next_field);
                let _ = writeln!(
                    fill,
                    "        v[{n}].name = {vname}; v[{n}].value = static_cast<int64_t>({}); v[{n}].fields = {vfields};",
                    v.value
                );
                next_variant += 1;
            }
            format!("::dz::Slice<const {variant}>(&v[{start}], {})", r.variants.len())
        };
        let _ = writeln!(
            fill,
            "        t[{id}].name = {name}; t[{id}].module_name = {module}; t[{id}].size = {}; \
             t[{id}].align = {}; t[{id}].kind = {kind}::{};\n        t[{id}].fields = {fields}; \
             t[{id}].variants = {variants}; t[{id}].elem.index = {}u; t[{id}].len = {}u; \
             t[{id}].mutable_ = {}; t[{id}].tag_offset = {};",
            r.size,
            r.align,
            r.kind,
            r.elem,
            r.len,
            r.mutable,
            if r.tag_offset.is_empty() { "0" } else { &r.tag_offset },
        );
    }

    let mut out = String::from(
        "// Generated by the Dazzle compiler: the type table (spec §13). Do not edit.\n",
    );
    for h in headers {
        let _ = writeln!(out, "#include \"{h}\"");
    }
    // `offsetof` on a type with constructors or a base is only conditionally supported, and
    // clang and g++ both support it for every layout Dazzle emits; the warning says nothing
    // about this program (invariant 7).
    out.push_str(
        "\n#if defined(__GNUC__)\n#pragma GCC diagnostic ignored \"-Winvalid-offsetof\"\n#endif\n\n",
    );
    out.push_str(&namespace_open("std"));
    for (n, s) in strings.iter().enumerate() {
        let _ = writeln!(out, "static const uint8_t dz_s{n}[] = {};", quote_cpp(s));
    }
    let types = rows.len().max(1);
    let fields = field_count.max(1);
    let variants = variant_count.max(1);
    let _ = write!(
        out,
        "\nstruct DzTypeTable {{\n    {info} t[{types}];\n    {field} f[{fields}];\n    {variant} v[{variants}];\n\
         \n    DzTypeTable() {{\n{fill}    }}\n}};\n\n\
         const void* dz_type_table_entry(uint32_t id) {{\n    static DzTypeTable table;\n    \
         if (id >= {count}u) {{\n        ::dz::panic(\"typeinfo of an id the type table does not hold\", \
         ::dz::SourceLocation{{\"<type table>\", \"typeinfo\", 0, 0}});\n    }}\n    return &table.t[id];\n}}\n",
        count = rows.len()
    );
    out.push_str(&namespace_close("std"));
    let _ = writeln!(
        out,
        "\nnamespace dz {{\nconst void* reflect_entry(uint32_t id) {{ return ::dz::{}::dz_type_table_entry(id); }}\n}}  // namespace dz",
        sanitize("std")
    );
    out
}

/// One type's row, interning the types it is made of.
fn describe(e: &mut Emitter<'_>, ty: &Ty) -> TypeRow {
    let cpp = e.table_spelling(ty);
    let mut row = TypeRow {
        name: dz_check::type_name(e.res, ty),
        size: format!("sizeof({cpp})"),
        align: format!("alignof({cpp})"),
        ..TypeRow::default()
    };
    let usize_ty = Ty::Int(IntTy::Usize);
    let field = |name: &str, ty: u32, offset: String| FieldRow {
        name: name.to_owned(),
        ty,
        offset,
        vis: "Public",
        owning: false,
        tag: String::new(),
    };
    row.kind = match ty {
        Ty::Void | Ty::Never | Ty::Error => {
            row.size = "0".to_owned();
            row.align = "1".to_owned();
            "Void"
        }
        Ty::Bool => "Bool",
        Ty::Char => "Char",
        Ty::Int(i) => int_kind(*i),
        Ty::IntLit => "I32",
        Ty::Float(FloatTy::F32) => "F32",
        Ty::Float(FloatTy::F64) | Ty::FloatLit => "F64",
        Ty::Str => {
            let p = e.reflect_id(&Ty::ptr(false, Ty::Int(IntTy::U8)));
            let n = e.reflect_id(&usize_ty);
            row.fields = vec![
                field("ptr", p, "offsetof(::dz::str, ptr)".to_owned()),
                field("len", n, "offsetof(::dz::str, len)".to_owned()),
            ];
            "Str"
        }
        Ty::CStr => "Cstr",
        Ty::Ptr { mutable, inner } | Ty::OwnPtr { mutable, inner } | Ty::Ref { mutable, inner } => {
            if matches!(ty, Ty::Ref { .. }) {
                row.size = "sizeof(void*)".to_owned();
                row.align = "alignof(void*)".to_owned();
                if e.table_is_trait(inner) {
                    row.size = format!("sizeof({cpp})");
                    row.align = format!("alignof({cpp})");
                    return TypeRow { kind: "TraitObject", ..row };
                }
            }
            row.elem = e.reflect_id(inner);
            row.mutable = *mutable;
            "Pointer"
        }
        Ty::Slice { mutable, elem } => {
            row.elem = e.reflect_id(elem);
            row.mutable = *mutable;
            let p = e.reflect_id(&Ty::ptr(*mutable, (**elem).clone()));
            let n = e.reflect_id(&usize_ty);
            row.fields = vec![
                field("ptr", p, format!("offsetof({cpp}, ptr)")),
                field("len", n, format!("offsetof({cpp}, len)")),
            ];
            "Slice"
        }
        Ty::Array { len, elem } => {
            row.elem = e.reflect_id(elem);
            row.len = *len;
            "Array"
        }
        Ty::Optional(inner) => {
            row.elem = e.reflect_id(inner);
            let b = e.reflect_id(&Ty::Bool);
            row.fields = vec![
                field("value", row.elem, format!("offsetof({cpp}, value)")),
                field("has", b, format!("offsetof({cpp}, has)")),
            ];
            "Optional"
        }
        Ty::Tuple(parts) => {
            for (k, p) in parts.iter().enumerate() {
                let id = e.reflect_id(p);
                row.fields.push(field(&k.to_string(), id, format!("offsetof({cpp}, v{k})")));
            }
            "Tuple"
        }
        Ty::Fn { .. } => "Function",
        Ty::Named { sym, args } => return describe_named(e, row, &cpp, *sym, args),
        // Values the language provides, the payload of one union variant, a range, a
        // fallible result: described by their size, with nothing to walk.
        _ => "Struct",
    };
    row
}

fn int_kind(i: IntTy) -> &'static str {
    match i {
        IntTy::I8 => "I8",
        IntTy::I16 => "I16",
        IntTy::I32 => "I32",
        IntTy::I64 => "I64",
        IntTy::Isize => "Isize",
        IntTy::U8 => "U8",
        IntTy::U16 => "U16",
        IntTy::U32 => "U32",
        IntTy::U64 => "U64",
        IntTy::Usize => "Usize",
    }
}

impl<'a> Emitter<'a> {
    fn table_is_trait(&self, t: &Ty) -> bool {
        matches!(t, Ty::Named { sym, .. } if self.res.symbol(*sym).kind == SymbolKind::Trait)
    }
}

fn describe_named(
    e: &mut Emitter<'_>,
    mut row: TypeRow,
    cpp: &str,
    sym: SymbolId,
    args: &[Ty],
) -> TypeRow {
    let symbol = e.res.symbol(sym);
    let foreign = e.ws.module(symbol.module).foreign.is_some();
    row.module = if foreign { String::new() } else { e.ws.module(symbol.module).name.clone() };
    let table: &TypeTable = e.table;
    match table.get(sym).cloned() {
        Some(ItemSig::Struct(sig)) => {
            row.kind = "Struct";
            let types = table.field_types(sym, args, &sig.fields);
            for (f, t) in sig.fields.iter().zip(types) {
                if f.bitfield {
                    continue;
                }
                let id = e.reflect_id(&t);
                let (owning, tag) = field_ast(e, f.declared_in.unwrap_or(sym), &f.name);
                row.fields.push(FieldRow {
                    name: f.name.clone(),
                    ty: id,
                    offset: format!("offsetof({cpp}, {})", member(e, sym, &f.name)),
                    vis: vis_name(f.vis),
                    owning,
                    tag,
                });
            }
        }
        Some(ItemSig::Enum { backing, variants, .. }) => {
            row.kind = "Enum";
            row.elem = e.reflect_id(&Ty::Int(backing));
            // An imported C enum's constants are the header's to spell; a Dazzle enum's
            // values are read off the C++ enum so they are exactly what is stored.
            if !foreign {
                for v in &variants {
                    row.variants.push(VariantRow {
                        name: v.name.clone(),
                        value: format!("{cpp}::{}", sanitize(&v.name)),
                        fields: Vec::new(),
                    });
                }
            }
        }
        Some(ItemSig::Union { variants, .. }) => {
            row.kind = "Union";
            row.elem = e.reflect_id(&Ty::Int(IntTy::U32));
            row.tag_offset = format!("offsetof({cpp}, kind)");
            for v in &variants {
                let vname = sanitize(&v.name);
                let base = format!("offsetof({cpp}, {vname})");
                let mut fields = Vec::new();
                match v.carries {
                    Some(carried) => {
                        let id = e.reflect_id(&Ty::plain(carried));
                        fields.push(FieldRow {
                            name: e.res.symbol(carried).name.clone(),
                            ty: id,
                            offset: base.clone(),
                            vis: "Public",
                            owning: false,
                            tag: String::new(),
                        });
                    }
                    None => {
                        let types = table.field_types(sym, args, &v.fields);
                        for (f, t) in v.fields.iter().zip(types) {
                            let id = e.reflect_id(&t);
                            let (owning, tag) = variant_field_ast(e, sym, &v.name, &f.name);
                            fields.push(FieldRow {
                                name: f.name.clone(),
                                ty: id,
                                offset: format!(
                                    "{base} + offsetof({cpp}::{vname}_t, {})",
                                    sanitize(&f.name)
                                ),
                                vis: "Public",
                                owning,
                                tag,
                            });
                        }
                    }
                }
                row.variants.push(VariantRow {
                    name: v.name.clone(),
                    value: format!("{cpp}::Kind::{vname}"),
                    fields,
                });
            }
        }
        // A trait is not a value: only a pointer or reference to one has a size, and that
        // pointer is described by its own row.
        _ => {
            row.kind = "TraitObject";
            row.size = "0".to_owned();
            row.align = "1".to_owned();
        }
    }
    row
}

/// A struct field's C++ member name: an imported one keeps the header's spelling.
fn member(e: &Emitter<'_>, sym: SymbolId, name: &str) -> String {
    if e.ws.module(e.res.symbol(sym).module).foreign.is_some() {
        name.to_owned()
    } else {
        sanitize(name)
    }
}

fn vis_name(v: dz_ast::Vis) -> &'static str {
    match v {
        dz_ast::Vis::Private => "Private",
        dz_ast::Vis::Protected => "Protected",
        dz_ast::Vis::Module => "Module",
        dz_ast::Vis::Public => "Public",
    }
}

/// A struct field's `own` and tag, read from its declaration.
fn field_ast(e: &Emitter<'_>, owner: SymbolId, name: &str) -> (bool, String) {
    let Some((file, decl)) = e.struct_asts.get(&owner) else { return (false, String::new()) };
    for m in &decl.members {
        if let dz_ast::MemberKind::Field(f) = &m.kind {
            if f.name.name == name {
                return (f.is_own, tag_text(e, *file, f.tag));
            }
        }
    }
    (false, String::new())
}

fn variant_field_ast(
    e: &Emitter<'_>,
    owner: SymbolId,
    variant: &str,
    name: &str,
) -> (bool, String) {
    let Some((file, decl)) = e.union_asts.get(&owner) else { return (false, String::new()) };
    for v in &decl.variants {
        if v.name.name != variant {
            continue;
        }
        for f in &v.fields {
            if f.name.name == name {
                return (f.is_own, tag_text(e, *file, f.tag));
            }
        }
    }
    (false, String::new())
}

/// A tag's text without its backticks.
fn tag_text(e: &Emitter<'_>, file: dz_span::FileId, tag: Option<dz_span::Span>) -> String {
    let Some(span) = tag else { return String::new() };
    let text = &e.map.get(file).text()[span.range()];
    text.trim_start_matches('`').trim_end_matches('`').to_owned()
}

pub(crate) type Shared = RefCell<Reflection>;
