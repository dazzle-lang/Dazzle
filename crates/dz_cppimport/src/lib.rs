//! The C and C++ header importer (spec §15).
//!
//! libclang parses a header exactly as the C++ compiler will when the generated code
//! includes it, and this crate turns what the header declares into [`Decl`]s — plain data
//! with no libclang in it, so the resolver and checker can build symbols from them and a
//! cache can hold them. The importer only teaches the compiler what exists: calls into a
//! header are emitted exactly as written (§15).
//!
//! What Dazzle cannot name yet is left out and recorded in [`Header::skipped`] with the
//! reason, never guessed at. M7a imports C: functions, structs, enums, typedefs naming a
//! struct or enum, and object-like macros whose value is a number. C++ is M7b (spec §19).

// libclang's constants keep their C names (`CXCursor_StructDecl`), which are matched on.
#![allow(non_upper_case_globals)]

use std::path::{Path, PathBuf};

// A WebAssembly build has no libclang to load, nor a C compiler's headers to read, so it
// carries the one header the standard library cannot do without (spec §14.2) and no other.
// Built everywhere, so a test can hold it to what libclang reads.
mod builtin;
#[cfg(not(target_family = "wasm"))]
mod clang;

#[cfg(target_family = "wasm")]
pub use builtin::import;
#[cfg(not(target_family = "wasm"))]
pub use clang::import;

/// The bytes of one of [`Header::files`]: from disk, or from the compiler itself for a
/// header a WebAssembly build carries.
pub fn read_file(path: &Path) -> std::io::Result<Vec<u8>> {
    #[cfg(target_family = "wasm")]
    if let Some(text) = builtin::text(path) {
        return Ok(text.as_bytes().to_vec());
    }
    std::fs::read(path)
}

// ---------------------------------------------------------------------------
// what an import produces
// ---------------------------------------------------------------------------

/// What one import read: a library's headers, parsed together as one unit, so what they
/// declare is one module (spec §15.1).
#[derive(Debug, Clone, PartialEq)]
pub struct Header {
    /// The headers as the recipe wrote them, in order: `["raylib.h", "raymath.h"]`.
    pub requested: Vec<String>,
    /// Where the include search found each of `requested`, in the same order.
    pub paths: Vec<PathBuf>,
    /// Where the include search found the first of them.
    pub path: PathBuf,
    /// The files declarations came from, the header first; [`Decl::file`] indexes this.
    pub files: Vec<PathBuf>,
    pub decls: Vec<Decl>,
    /// Declarations that exist in C but not yet in Dazzle, and why.
    pub skipped: Vec<Skipped>,
}

impl Header {
    pub fn decl(&self, name: &str) -> Option<&Decl> {
        self.decls.iter().find(|d| d.name == name)
    }
}

/// A byte range in one of [`Header::files`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Loc {
    pub file: u32,
    pub start: u32,
    pub end: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Decl {
    /// The name Dazzle knows it by — the C++ name unqualified, or the name
    /// `[cpp.instantiate]` gave a template instance (spec §15.4).
    pub name: String,
    /// The enclosing C++ namespaces, outermost first. Empty in a C header (spec §15.2).
    pub scope: Vec<String>,
    /// Exactly what the generated C++ writes for it: `::InitWindow`, `::JPH::Body`,
    /// `::std::vector<float>`. Kept beside the name because the two differ as soon as
    /// namespaces or template instances are involved, and codegen must never guess.
    pub cpp: String,
    pub loc: Loc,
    /// The documentation comment written above it in the header, markers stripped — what an
    /// editor shows on hover. Doxygen's `@param`/`\brief` are kept as written: they are what
    /// the header's author wrote, and rewriting them would lose information.
    pub docs: String,
    pub kind: DeclKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DeclKind {
    /// `ret_cast` is the Dazzle-side C++ type a returned pointer must be cast back to,
    /// for the same reason as [`Param::c_cast`].
    Fn { params: Vec<Param>, ret: CType, ret_cast: bool },
    /// `opaque` when the header declares the struct and never defines it — `RGFW_window`,
    /// `FILE`. Its size is unknown, so it is a handle: usable behind a pointer and never by
    /// value, and it has no fields to name (spec §15.2, §18.40).
    ///
    /// `is_union` for a C `union`, whose members are the same memory rather than fields
    /// beside each other. Dazzle's own `union` is tagged (§7.5), which a C one is not, so it
    /// comes in as a type whose members overlap exactly as they do in C (§18.41).
    Struct {
        fields: Vec<Field>,
        opaque: bool,
        is_union: bool,
        /// A C++ class's public member functions, in declaration order (spec §15.2). Empty
        /// for a C struct, which has none.
        methods: Vec<Method>,
        /// The class has a destructor C++ runs itself, which makes it move-only in Dazzle
        /// unless it also has a copy constructor (spec §15.2).
        move_only: bool,
    },
    /// An unscoped enum's enumerators are *also* imported as [`DeclKind::Const`]s at the
    /// header's scope, since C code uses them as plain integers (spec §15.2).
    Enum { scoped: bool, backing: CInt, variants: Vec<Enumerator> },
    /// `typedef Texture Texture2D`: a second name for a struct or enum.
    Alias { target: String },
    /// An unscoped enumerator, or an object-like macro that is a number.
    Const { value: ConstValue, from_macro: bool },
}

/// A C++ member function, which Dazzle calls as a method (spec §15.2).
#[derive(Debug, Clone, PartialEq)]
pub struct Method {
    pub name: String,
    pub loc: Loc,
    pub params: Vec<Param>,
    pub ret: CType,
    pub ret_cast: bool,
    /// A `const` member function reads its receiver and takes `&self`; any other takes
    /// `&mut self`. `None` is a `static` member function, called through the type.
    pub receiver: Option<Receiver>,
    /// The comment above it in the header (see [`Decl::docs`]).
    pub docs: String,
    /// A constructor, which Dazzle calls as `Class(args)` (spec §15.2).
    pub is_ctor: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Receiver {
    Ref,
    MutRef,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: String,
    pub ty: CType,
    /// The C spelling to cast to when Dazzle's own is a different C++ type: `char *` is not
    /// `int8_t *` (that is `signed char *`), and `long *` is not `int32_t *` even where the
    /// two are the same width. Pointers do not convert between them (spec §15.2).
    pub c_cast: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub name: String,
    pub ty: CType,
    pub loc: Loc,
    /// The comment above it in the header (see [`Decl::docs`]).
    pub docs: String,
    /// `unsigned flags : 3` — read and written like any other member, but C++ forbids
    /// taking its address, so Dazzle does too (spec §15.2).
    pub bitfield: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Enumerator {
    pub name: String,
    pub value: i128,
    pub loc: Loc,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConstValue {
    Int(i128),
    /// `single` when written with an `f` suffix, which makes it a C `float`.
    Float {
        value: f64,
        single: bool,
    },
    /// A macro whose value is not a number but which clang can give a type — raylib's
    /// `RAYWHITE`, a `Color` compound literal. Emitted as the macro's own name (§15.2).
    Typed(CType),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CInt {
    pub bits: u8,
    pub signed: bool,
}

/// A C type, in the terms Dazzle maps it to (spec §15.2).
#[derive(Debug, Clone, PartialEq)]
pub enum CType {
    Void,
    Bool,
    Int(CInt),
    /// `size_t` and `uintptr_t` (unsigned), `ptrdiff_t` and `intptr_t` (signed): Dazzle's
    /// `usize` and `isize` rather than whatever width they have here.
    Size {
        signed: bool,
    },
    Float {
        bits: u8,
    },
    /// `const char *`.
    CStr,
    Ptr {
        mutable: bool,
        pointee: Box<CType>,
    },
    /// `float m[16]` — a fixed-size array, as a struct member has one.
    Array {
        len: u64,
        elem: Box<CType>,
    },
    /// `void (*)(const RGFW_event *)` — a callback, which is Dazzle's `fn(…) -> T`.
    Fn {
        params: Vec<CType>,
        ret: Box<CType>,
    },
    /// `const T&` and `T&`, which are Dazzle's `&T` and `&mut T` (spec §15.2). A C++ API
    /// takes one almost everywhere a C API would take a pointer.
    Ref {
        mutable: bool,
        referent: Box<CType>,
    },
    /// A struct or enum imported from the same header, by the name it was imported under.
    Named(String),
    /// Something Dazzle cannot name yet, as C spells it.
    Unsupported(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Skipped {
    pub name: String,
    pub loc: Loc,
    pub reason: String,
}

/// Why a header could not be imported at all.
#[derive(Debug, Clone, PartialEq)]
pub enum ImportError {
    /// libclang could not be found or loaded.
    Unavailable(String),
    /// clang could not read the header — it was not found, or it has errors. Formatted
    /// clang diagnostics, errors only.
    Failed(Vec<String>),
}

/// What to import, and how the header is to be read: with exactly the flags the generated
/// C++ is compiled with, from the directory it is compiled in (spec §15.1).
pub struct Request<'a> {
    /// The headers to read, as `#include <…>` names them; several are parsed as one unit.
    pub headers: &'a [String],
    pub include_dirs: &'a [PathBuf],
    pub defines: &'a [String],
    pub cpp_standard: &'a str,
    /// Where the generated C++ lives, so a quoted `#include` is searched for from the same
    /// place the C++ compiler will search it from.
    pub from_dir: &'a Path,
    /// `[cpp.instantiate]`: the C++ type to name, and the Dazzle name to import it under.
    /// Templates are not mapped onto Dazzle generics; the instantiations a project asks for
    /// are imported as ordinary concrete types (spec §15.4).
    pub instantiate: &'a [(String, String)],
}

#[cfg(test)]
mod tests {
    use super::clang::{clang_major, find_resource_dir, number};
    use super::ConstValue;
    use std::path::{Path, PathBuf};

    #[test]
    fn the_major_version_is_read_whoever_built_clang() {
        assert_eq!(clang_major("clang version 23.1.3").as_deref(), Some("23"));
        assert_eq!(clang_major("Ubuntu clang version 18.1.3 (1ubuntu1)").as_deref(), Some("18"));
        assert_eq!(
            clang_major("Apple clang version 17.0.0 (clang-1700.0.13.3)").as_deref(),
            Some("17")
        );
        assert_eq!(clang_major("clang"), None);
    }

    /// A fake LLVM layout under the system's temp directory, so these tests read nothing
    /// that is installed on the machine running them.
    struct Layout(PathBuf);

    impl Layout {
        fn new(name: &str) -> Layout {
            let root =
                std::env::temp_dir().join(format!("dz_resdir_{name}_{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root).unwrap();
            Layout(root)
        }

        fn file(&self, rel: &str) -> PathBuf {
            let p = self.0.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, "").unwrap();
            p
        }

        fn found(&self, lib: &Path, version: &str) -> Option<String> {
            let d = find_resource_dir(lib, version)?;
            Some(d.strip_prefix(&self.0).unwrap().to_string_lossy().replace('\\', "/"))
        }
    }

    impl Drop for Layout {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn the_resource_directory_is_beside_the_library() {
        let l = Layout::new("beside");
        let lib = l.file("usr/lib/libclang.so.23.1");
        l.file("usr/lib/clang/23/include/stddef.h");
        // Another version's headers are never taken.
        l.file("usr/lib/clang/22/include/stddef.h");
        assert_eq!(l.found(&lib, "clang version 23.1.3").as_deref(), Some("usr/lib/clang/23"));
        assert_eq!(l.found(&lib, "clang version 21.1.8"), None);
    }

    #[test]
    fn a_library_in_bin_finds_the_sibling_lib() {
        let l = Layout::new("bin");
        let lib = l.file("LLVM/bin/libclang.dll");
        l.file("LLVM/lib/clang/21/include/stddef.h");
        assert_eq!(l.found(&lib, "clang version 21.1.0").as_deref(), Some("LLVM/lib/clang/21"));
    }

    #[test]
    fn a_directory_named_by_the_full_version_counts() {
        let l = Layout::new("apple");
        let lib = l.file("usr/lib/libclang.dylib");
        l.file("usr/lib/clang/15.0.0/include/stddef.h");
        assert_eq!(
            l.found(&lib, "Apple clang version 15.0.0 (clang-1500.3.9.4)").as_deref(),
            Some("usr/lib/clang/15.0.0")
        );
    }

    /// A WebAssembly build knows `stdlib.h` only as `builtin.rs` writes it, so what it says
    /// of each function must be what libclang reads from the real header.
    #[test]
    fn the_carried_stdlib_says_what_the_real_one_does() {
        let req = super::Request {
            headers: &["stdlib.h".to_owned()],
            include_dirs: &[],
            defines: &[],
            cpp_standard: "c++17",
            from_dir: &std::env::temp_dir(),
            instantiate: &[],
        };
        let real = match super::clang::import(&req) {
            Ok(h) => h,
            Err(super::ImportError::Unavailable(e)) => {
                eprintln!("skipped: libclang is not available ({e})");
                return;
            }
            Err(e) => panic!("stdlib.h did not import: {e:?}"),
        };
        // Where it is, what its comment says and what its parameters are called differ by C
        // library (`__size`, `_Size`); what it is cannot.
        let unnamed = |kind: &super::DeclKind| match kind {
            super::DeclKind::Fn { params, ret, ret_cast } => super::DeclKind::Fn {
                params: params
                    .iter()
                    .map(|p| super::Param { name: String::new(), ..p.clone() })
                    .collect(),
                ret: ret.clone(),
                ret_cast: *ret_cast,
            },
            other => other.clone(),
        };
        for carried in super::builtin::stdlib().decls {
            let found = real.decl(&carried.name).expect("the real stdlib.h declares it");
            assert_eq!(
                (&found.cpp, &found.scope, unnamed(&found.kind)),
                (&carried.cpp, &carried.scope, unnamed(&carried.kind)),
                "`{}`",
                carried.name
            );
        }
    }

    #[test]
    fn a_directory_without_the_headers_does_not_count() {
        let l = Layout::new("empty");
        let lib = l.file("usr/lib/libclang.so");
        std::fs::create_dir_all(l.0.join("usr/lib/clang/23/include")).unwrap();
        assert_eq!(l.found(&lib, "clang version 23.1.3"), None);
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_library_is_followed_to_where_it_lives() {
        let l = Layout::new("symlink");
        let real = l.file("usr/lib/llvm21/lib/libclang.so.21.1");
        l.file("usr/lib/llvm21/lib/clang/21/include/stddef.h");
        let link = l.0.join("usr/lib/libclang.so.21.1");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        // `canonicalize` resolves the temp directory too (`/tmp` is a link on macOS), so
        // compare against the canonical root.
        let found = find_resource_dir(&link, "clang version 21.1.8").unwrap();
        let root = std::fs::canonicalize(&l.0).unwrap();
        assert_eq!(found, root.join("usr/lib/llvm21/lib/clang/21"));
    }

    #[test]
    fn a_macro_is_a_constant_only_when_it_is_a_number() {
        assert_eq!(number(&["7"]), Some(ConstValue::Int(7)));
        assert_eq!(number(&["0x10u"]), Some(ConstValue::Int(16)));
        assert_eq!(number(&["010"]), Some(ConstValue::Int(8)));
        assert_eq!(number(&["(", "-", "1", ")"]), Some(ConstValue::Int(-1)));
        assert_eq!(number(&["0.5f"]), Some(ConstValue::Float { value: 0.5, single: true }));
        assert_eq!(number(&["1e3"]), Some(ConstValue::Float { value: 1000.0, single: false }));
        assert_eq!(number(&["1", "<<", "3"]), None);
        assert_eq!(number(&["\"text\""]), None);
    }
}
