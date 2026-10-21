# 13. Reflection

Reflection is **a compile-time-generated static table**, in the Odin spirit: your data
carries nothing, and the metadata lives in read-only memory beside it. `[DECIDED]`

## 13.1 The type table

The compiler writes one table per program. A `TypeId` is an index into it, and **nothing in
your structs ever stores one unless you put it there yourself**. The table's types are
ordinary declarations of the standard library (§14.4):

```dz
pub struct TypeId {
    pub index: u32
}

pub enum TypeKind : u8 {
    Void  Bool  Char  I8  I16  I32  I64  Isize  U8  U16  U32  U64  Usize  F32  F64
    Str  Cstr  Pointer  Slice  Array  Optional  Tuple  Function  TraitObject
    Struct  Union  Enum
}

pub struct TypeInfo {
    pub name:       str             // `Transform`, `List<i32>`, `[]u8` — as Dazzle writes it
    pub module_name: str            // the declaring module of a named type; empty otherwise
    pub size:       usize
    pub align:      usize
    pub kind:       TypeKind
    pub fields:     []FieldInfo     // what a value is made of; see below
    pub variants:   []VariantInfo   // an enum's or a union's
    pub elem:       TypeId          // what a pointer, slice, array or optional holds; an enum's backing type
    pub len:        usize           // an array's length
    pub mutable:    bool            // a `^mut`, `&mut` or `[]mut`
    pub tag_offset: usize           // where a union keeps which variant it holds (a `u32`)
}

pub struct FieldInfo {
    pub name:    str
    pub type_id: TypeId
    pub offset:  usize              // bytes from the start of the value
    pub vis:     Visibility         // Private, Protected, Module, Public
    pub owning:  bool               // an `own` field
    pub tag:     str                // the field's tag, `json:"hp"`, or empty

    pub fn tag_value(&self, key: str) -> str?   // `f.tag_value("json")` → `"hp"`
}

pub struct VariantInfo {
    pub name:   str
    pub value:  i64                 // an enum variant's value; a union variant's tag
    pub fields: []FieldInfo         // a union variant's payload, offsets from the union's start
}

pub fn typeinfo_of<T>() -> &TypeInfo    // resolved at compile time; a static reference
pub fn typeid_of<T>()   -> TypeId
pub fn typeinfo(id: TypeId) -> &TypeInfo
```

**Every kind of type is described**, so generic code can walk any value it is handed
`[DECIDED — §18.59]`:

| `kind` | What else it fills in |
|---|---|
| the primitives, `Void` … `F64` | nothing: `size` says how many bytes to read |
| `Str`, `Cstr` | `fields`: `ptr` and `len` for a `str`; a `cstr` is one pointer |
| `Pointer` | `elem`, `mutable` — `^T`, `^mut T`, `&T`, `&mut T` and `own` pointers alike |
| `Slice` | `elem`, `mutable`, and `fields`: `ptr`, `len` |
| `Array` | `elem`, `len`: the elements are contiguous, `elem`'s size apart |
| `Optional` | `elem`, and `fields`: `value` and `has`, a `bool` saying whether it is there |
| `Tuple` | `fields` named `0`, `1`, … |
| `Function`, `TraitObject` | nothing more |
| `Struct` | `fields`: its own and every base's, bases first (§7.3) |
| `Enum` | `variants` with their values, and `elem`, the backing integer type |
| `Union` | `variants`, each with its payload's fields — a variant carrying an existing struct has one field, named after that struct — and `tag_offset` |

Every field is described, private ones included, with the visibility it was declared with:
the table describes memory, and what code may *name* is still §4's business. A field whose
address C++ cannot take — an imported C bit-field (§15.2) — is left out.

**Field tags.** A field may end in a raw string, its *tag*: free-form text that means
nothing to the compiler and is carried into the table for the library that reads it.
`key:"value"` pairs separated by spaces are the convention, as in Odin and Go, and
`FieldInfo::tag_value` looks one up:

```dz
pub struct Player {
    pub name:   String              `json:"player_name"`
    pub hp:     i32 = 100           `json:"health" save:"-"`
    m_session:  u64
}
```

The tag comes after the type and the default. `[DECIDED — §18.59]`

## 13.2 What gets included

Controlled by `type_info` in `dazzle.toml`:

- `"none"` — no table at all. Smallest possible binary; a reflection call is an error
  (`DZ0555`).
- `"used"` *(default)* — a type is in the table if the program calls `typeinfo_of<T>()` or
  `typeid_of<T>()` on it, or reaches it from such a type: through its fields, variants and
  element types. This is the pay-for-what-you-use setting. There is no attribute to opt a
  type in: code that must find types it does not name — an editor listing every component —
  keeps their ids itself, `let components = [typeid_of<Transform>(), typeid_of<Health>()]`,
  which is also what puts them in the table.
- `"all"` — every non-generic type the program declares, and every instance of a generic
  one it uses. What an editor build wants.

## 13.3 Reading and writing fields

Because a `FieldInfo` carries a byte offset and a `TypeId`, generic code can walk a value it
does not know the type of:

```dz
pub fn write_json(out: &mut String, value: ^u8, id: TypeId) {
    let info = typeinfo(id)
    switch info.kind {
        case Struct {
            out.push('{')
            for &f in info.fields {
                let key = f.tag_value("json") or f.name
                out.push_str(format!("\"{}\":", key).as_str())
                write_json(out, unsafe { ((value as usize) + f.offset) as ^u8 }, f.type_id)
            }
            out.push('}')
        }
        case I32 { out.push_str(format!("{}", unsafe { (value as ^i32)^ }).as_str()) }
        // ...
    }
}
```

That is the whole serializer, and it is the same code that drives an editor inspector, a
network replication layer, and a save system.

## 13.4 Compile-time reflection

A macro generates specialised code per field with no runtime table lookup at all. Macros
expand before types exist (§12.11), so such a macro takes the declaration itself and reads
its fields from what was written:

```dz
macro serializable -> decl {
    syntax Field { $name: ident : $type: type $( = $default: expr )? }

    pattern { struct $name: ident { $fields: []Field } }

    expand {
        struct $name {
            #for f in $fields { pub $f.name: $f.type #if $f.default { = $f.default } }
        }

        impl Serialize for $name {
            fn serialize(&self, w: &mut Writer) {
                #for f in $fields {
                    w.key(#expr_text($f.name))
                    $f.name.serialize(w)
                }
            }
        }
    }
}

#serializable struct Transform {
    position: Vec3
    scale: f32 = 1
}
```

The two mechanisms are complementary: a macro generates fast, specialised code for types
you declare through it; the runtime table handles every type, including those you only know
by `TypeId`. Iterating `T::fields` of a type *parameter* at compile time is deferred
(§12.11).

## 13.5 Mapping back to the C++ output

Field offsets in the table are computed by the C++ compiler, with `offsetof`, so the table is
correct under any padding, packing, or ABI the target uses. It is filled in once, the first
time it is read, rather than by a global constructor (§16.4). Type names are the
Dazzle-level names — `Transform`, `List<i32>`, with the module in `module_name` — never the
mangled C++ ones.
