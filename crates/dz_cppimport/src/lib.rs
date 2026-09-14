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

use std::collections::{HashMap, HashSet};
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_uint, c_ulong};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::SystemTime;

use clang_sys::*;

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

// ---------------------------------------------------------------------------
// the entry point, and its cache
// ---------------------------------------------------------------------------

/// A file an import read, as it was when it was read.
#[derive(Debug, Clone, PartialEq)]
struct Stamp {
    path: PathBuf,
    len: u64,
    modified: Option<SystemTime>,
}

impl Stamp {
    fn of(path: &Path) -> Stamp {
        let meta = std::fs::metadata(path).ok();
        Stamp {
            path: path.to_path_buf(),
            len: meta.as_ref().map_or(0, |m| m.len()),
            modified: meta.and_then(|m| m.modified().ok()),
        }
    }
}

type Cache = Mutex<HashMap<String, (Arc<Header>, Vec<Stamp>)>>;

/// One process, one cache: the language server re-analyses on every keystroke, and a
/// header nobody touched must not be parsed again. An entry holds while every file the
/// header pulled in — its own includes, system headers too — is unchanged on disk.
fn cache() -> &'static Cache {
    static CACHE: OnceLock<Cache> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Import one header.
pub fn import(req: &Request<'_>) -> Result<Arc<Header>, ImportError> {
    let args = arguments(req);
    // The instantiations belong in the key: they change what the header imports, and two
    // modules asking for different ones must not share an entry (spec §15.4).
    let wanted =
        req.instantiate.iter().map(|(c, n)| format!("{c}={n}")).collect::<Vec<_>>().join("\0");
    let key = format!(
        "{}\0{}\0{}\0{wanted}",
        req.headers.join("\u{1}"),
        req.from_dir.display(),
        args.join("\0")
    );
    if let Ok(map) = cache().lock() {
        if let Some((header, stamps)) = map.get(&key) {
            if stamps.iter().all(|s| *s == Stamp::of(&s.path)) {
                return Ok(header.clone());
            }
        }
    }
    ensure_loaded().map_err(ImportError::Unavailable)?;
    // SAFETY: libclang is loaded on this thread, and everything handed to it outlives the
    // calls; `parse` disposes of what it creates on every path out.
    let (header, inputs) = unsafe { parse(req, &args)? };
    let header = Arc::new(header);
    let stamps = inputs.iter().map(|p| Stamp::of(p)).collect();
    if let Ok(mut map) = cache().lock() {
        map.insert(key, (header.clone(), stamps));
    }
    Ok(header)
}

fn arguments(req: &Request<'_>) -> Vec<String> {
    // Parsed as C++, because the generated C++ is what includes it: a C header has to
    // survive that anyway, and its `extern "C"` guard is only visible this way.
    let mut args = vec!["-x".to_owned(), "c++".to_owned(), format!("-std={}", req.cpp_standard)];
    if let Some(sdk) = macos_sysroot() {
        // On macOS the C and C++ standard headers live inside the active SDK rather than
        // in `/usr/include`, and libclang does not run `xcrun` to find it the way the
        // `clang` driver does. Without this, importing any header that includes a system
        // one fails with "'assert.h' file not found" — which names the wrong problem
        // entirely (spec §15.1).
        args.push("-isysroot".to_owned());
        args.push(sdk);
    }
    args.extend(req.include_dirs.iter().map(|d| format!("-I{}", d.display())));
    args.extend(req.defines.iter().map(|d| format!("-D{d}")));
    args
}

/// The active macOS SDK, asked for once. `None` everywhere else, and on a macOS without
/// the command line tools — where there is nothing to point at anyway.
fn macos_sysroot() -> Option<String> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    static SDK: OnceLock<Option<String>> = OnceLock::new();
    SDK.get_or_init(|| {
        // `SDKROOT` is what Xcode itself sets, so honour it before shelling out.
        if let Some(root) = std::env::var_os("SDKROOT") {
            let root = root.to_string_lossy().into_owned();
            if !root.is_empty() && Path::new(&root).is_dir() {
                return Some(root);
            }
        }
        let out = std::process::Command::new("xcrun").args(["--show-sdk-path"]).output().ok()?;
        if !out.status.success() {
            return None;
        }
        let path = String::from_utf8_lossy(&out.stdout).trim().to_owned();
        (!path.is_empty() && Path::new(&path).is_dir()).then_some(path)
    })
    .clone()
}

// ---------------------------------------------------------------------------
// loading libclang
// ---------------------------------------------------------------------------

/// The library is loaded once and shared: clang-sys keeps it per thread, and the language
/// server analyses on more than one.
fn ensure_loaded() -> Result<(), String> {
    if clang_sys::is_loaded() {
        return Ok(());
    }
    static SHARED: OnceLock<Result<Arc<clang_sys::SharedLibrary>, String>> = OnceLock::new();
    let shared = SHARED.get_or_init(|| {
        // Where libclang is looked for, nearest first. `LIBCLANG_PATH` set by hand wins;
        // after that comes **the one shipped with `dz`**, so a release that carries its own
        // copy needs nothing installed on the machine; then an LLVM install, which anyone
        // who can build a Dazzle program with clang already has.
        if std::env::var_os("LIBCLANG_PATH").is_none() {
            if let Some(dir) = bundled().or_else(beside_clang) {
                std::env::set_var("LIBCLANG_PATH", dir);
            }
        }
        clang_sys::load()?;
        clang_sys::get_library().ok_or_else(|| "libclang did not stay loaded".to_owned())
    });
    match shared {
        Ok(lib) => {
            clang_sys::set_library(Some(lib.clone()));
            Ok(())
        }
        Err(e) => Err(e.clone()),
    }
}

/// The platform's file name for libclang.
fn lib_name() -> &'static str {
    if cfg!(windows) {
        "libclang.dll"
    } else if cfg!(target_os = "macos") {
        "libclang.dylib"
    } else {
        "libclang.so"
    }
}

/// A libclang shipped **with `dz` itself**, beside the executable or in a `lib/` next to
/// it — the layout a release archive unpacks to:
///
/// ```text
/// dz/
///   bin/dz            (or dz.exe)
///   lib/libclang.so   (or .dll / .dylib)
/// ```
///
/// This is what lets a distributed compiler read C headers on a machine with no LLVM
/// installed: the only thing `dz` then asks of the system is a C++ compiler to hand its
/// generated code to, which is a tool the user chooses anyway (spec §15.1, §16.5).
///
/// Nothing is bundled by the `cargo build` used during development, so this finds nothing
/// and the search falls through to an installed LLVM.
fn bundled() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let candidates = [dir.to_path_buf(), dir.join("lib"), dir.parent().map(|p| p.join("lib"))?];
    candidates.into_iter().find(|d| d.join(lib_name()).is_file())
}

fn beside_clang() -> Option<PathBuf> {
    let lib = lib_name();
    let exe = if cfg!(windows) { "clang.exe" } else { "clang" };
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).find(|d| d.join(exe).is_file() && d.join(lib).is_file())
}

// ---------------------------------------------------------------------------
// parsing
// ---------------------------------------------------------------------------

unsafe fn parse(req: &Request<'_>, args: &[String]) -> Result<(Header, Vec<PathBuf>), ImportError> {
    let index = clang_createIndex(0, 0);
    // The header is reached through an `#include` in a file that does not exist, placed
    // where the generated C++ will be, so the search is the C++ compiler's search.
    let main = req.from_dir.join("__dz_import__.cpp");
    let c_main = cstring(&main.to_string_lossy());
    // Angle brackets, for the reason `dz_cpp` emits them: the generated C++ lives in
    // `from_dir`, and a quoted include is searched from there first — so a module named
    // after the library it imports (`raylib`) would have the importer read the *generated*
    // `raylib.h` instead of the real one, and only once a previous build had left it there.
    // The two sides must name the same file or the compiler would check one header and
    // compile against another (spec §15.1).
    let contents = cstring(&includes(req.headers.iter().map(|h| format!("<{h}>"))));
    let mut unsaved = CXUnsavedFile {
        Filename: c_main.as_ptr(),
        Contents: contents.as_ptr(),
        Length: contents.as_bytes().len() as c_ulong,
    };
    let c_args: Vec<CString> = args.iter().map(|a| cstring(a)).collect();
    let argv: Vec<*const c_char> = c_args.iter().map(|a| a.as_ptr()).collect();
    let mut tu: CXTranslationUnit = std::ptr::null_mut();
    let code = clang_parseTranslationUnit2(
        index,
        c_main.as_ptr(),
        argv.as_ptr(),
        argv.len() as c_int,
        &mut unsaved,
        1,
        CXTranslationUnit_DetailedPreprocessingRecord | CXTranslationUnit_SkipFunctionBodies,
        &mut tu,
    );
    if code != CXError_Success || tu.is_null() {
        clang_disposeIndex(index);
        return Err(ImportError::Failed(vec![format!(
            "libclang could not parse `{}` (error {code})",
            req.headers.join("`, `")
        )]));
    }

    let errors = diagnostics(tu);
    let result = if errors.is_empty() {
        let mut w = Walker::default();
        w.walk(tu);
        let (inputs, direct) = inclusions(tu);
        // Each requested header by where it was found: the files the unsaved main file
        // includes itself, matched by the name the include wrote.
        let paths: Vec<PathBuf> = req
            .headers
            .iter()
            .map(|h| {
                let written = h.replace('\\', "/");
                direct
                    .iter()
                    .find(|p| p.to_string_lossy().replace('\\', "/").ends_with(&written))
                    .cloned()
                    .unwrap_or_else(|| PathBuf::from(h))
            })
            .collect();
        w.direct = paths.iter().map(|p| absolute(p)).collect();
        type_macros(&mut w, index, &c_main, &c_args);
        instantiate(&mut w, index, &c_main, &c_args, req.instantiate);
        let path = paths.first().cloned().unwrap_or_default();
        let header = Header {
            requested: req.headers.to_vec(),
            paths,
            path,
            files: w.files,
            decls: w.decls,
            skipped: w.skipped,
        };
        Ok((header, inputs))
    } else {
        Err(ImportError::Failed(errors))
    };
    clang_disposeTranslationUnit(tu);
    clang_disposeIndex(index);
    result
}

/// What a macro that is not a plain number expands to, if clang can say.
///
/// Each candidate is written out as `auto __dz_macro_X = (X);` beside the header and the
/// lot parsed once more. A macro whose expansion is a constant keeps its value — so
/// `(1 << 3)` is an untyped integer like any other (§18.38) — and one that is merely
/// well-typed, raylib's `RAYWHITE`, keeps its type and is emitted by name. Whatever does
/// not compile leaves no variable behind and is recorded as skipped (spec §15.2).
unsafe fn type_macros(w: &mut Walker, index: CXIndex, main: &CString, args: &[CString]) {
    /// A header with thousands of macros is not worth a second parse of each.
    const LIMIT: usize = 512;

    let candidates: Vec<(String, Loc)> = w.macro_candidates.drain(..).take(LIMIT).collect();
    if candidates.is_empty() {
        return;
    }
    let Some(mut source) = w.header_includes() else { return };
    for (name, _) in &candidates {
        source.push_str(&format!("auto __dz_macro_{name} = ({name});\n"));
    }

    let contents = cstring(&source);
    let mut unsaved = CXUnsavedFile {
        Filename: main.as_ptr(),
        Contents: contents.as_ptr(),
        Length: contents.as_bytes().len() as c_ulong,
    };
    let argv: Vec<*const c_char> = args.iter().map(|a| a.as_ptr()).collect();
    let mut tu: CXTranslationUnit = std::ptr::null_mut();
    let code = clang_parseTranslationUnit2(
        index,
        main.as_ptr(),
        argv.as_ptr(),
        argv.len() as c_int,
        &mut unsaved,
        1,
        CXTranslationUnit_SkipFunctionBodies,
        &mut tu,
    );
    if code != CXError_Success || tu.is_null() {
        for (name, loc) in candidates {
            w.skip(name, loc, "a macro whose value is not a number");
        }
        return;
    }

    let mut typed: HashMap<String, ConstValue> = HashMap::new();
    for c in children(clang_getTranslationUnitCursor(tu)) {
        if clang_getCursorKind(c) != CXCursor_VarDecl {
            continue;
        }
        let spelling = string(clang_getCursorSpelling(c));
        let Some(name) = spelling.strip_prefix("__dz_macro_") else { continue };
        // The variable is declared `auto`, whose declared type is the placeholder; the
        // canonical type is what it was deduced to.
        let ty = clang_getCanonicalType(clang_getCursorType(c));
        let mapped = w.map_type(ty);
        let value = match evaluate(c) {
            Some(v) => v,
            None => match mapped {
                CType::Unsupported(_) => continue,
                other => ConstValue::Typed(other),
            },
        };
        typed.insert(name.to_owned(), value);
    }
    clang_disposeTranslationUnit(tu);

    for (name, loc) in candidates {
        match typed.remove(&name) {
            Some(value) if w.claim(&name) => {
                let kind = DeclKind::Const { value, from_macro: true };
                w.push(name, loc, kind);
            }
            Some(_) => {}
            None => w.skip(name, loc, "a macro whose value Dazzle cannot name"),
        }
    }
}

/// The template instantiations `[cpp.instantiate]` asked for (spec §15.4).
///
/// Templates are not mapped onto Dazzle generics. Each wanted instantiation is written out
/// beside the header as `using __dz_inst_<Name> = <C++ type>;`, parsed, and the concrete
/// class it names imported as an ordinary type under the Dazzle name — which is why one
/// instantiation is one type, with the members it actually has for those arguments.
unsafe fn instantiate(
    w: &mut Walker,
    index: CXIndex,
    main: &CString,
    args: &[CString],
    wanted: &[(String, String)],
) {
    if wanted.is_empty() {
        return;
    }
    let Some(mut source) = w.header_includes() else { return };
    for (cpp, name) in wanted {
        source.push_str(&format!("using __dz_inst_{name} = {cpp};\n"));
        // An **explicit** instantiation definition, which is what puts the members in the
        // AST: libclang exposes no children for an implicit specialization, so merely
        // naming the type — or asking for its size — yields a class with nothing in it.
        source.push_str(&format!("template class {cpp};\n"));
    }

    let contents = cstring(&source);
    let mut unsaved = CXUnsavedFile {
        Filename: main.as_ptr(),
        Contents: contents.as_ptr(),
        Length: contents.as_bytes().len() as c_ulong,
    };
    let argv: Vec<*const c_char> = args.iter().map(|a| a.as_ptr()).collect();
    let mut tu: CXTranslationUnit = std::ptr::null_mut();
    let code = clang_parseTranslationUnit2(
        index,
        main.as_ptr(),
        argv.as_ptr(),
        argv.len() as c_int,
        &mut unsaved,
        1,
        CXTranslationUnit_None,
        &mut tu,
    );
    // The instantiation is named in `dazzle.toml`, not written in the header, so a
    // declaration made from it points at the header's first byte rather than nowhere.
    let anchor = Loc { file: 0, start: 0, end: 0 };
    if code != CXError_Success || tu.is_null() {
        for (cpp, name) in wanted {
            w.skip(name.clone(), anchor, format!("`{cpp}` could not be instantiated"));
        }
        return;
    }

    let mut found: HashSet<String> = HashSet::new();
    for c in children(clang_getTranslationUnitCursor(tu)) {
        if !matches!(clang_getCursorKind(c), CXCursor_TypeAliasDecl | CXCursor_TypedefDecl) {
            continue;
        }
        let alias = string(clang_getCursorSpelling(c));
        let Some(name) = alias.strip_prefix("__dz_inst_") else { continue };
        let under = clang_getCanonicalType(clang_getTypedefDeclUnderlyingType(c));
        let decl = clang_getTypeDeclaration(under);
        if !matches!(
            clang_getCursorKind(decl),
            CXCursor_StructDecl | CXCursor_ClassDecl | CXCursor_UnionDecl | CXCursor_ClassTemplate
        ) {
            continue;
        }
        found.insert(name.to_owned());
        if !w.seen.insert(name.to_owned()) {
            continue;
        }
        // libclang exposes no children for an *implicit* specialization — `Pair<int>` is a
        // class with nothing in it — so the members are read from the template it came
        // from, with its parameters bound to this instantiation's arguments. A member whose
        // type does not follow from that binding (`typename T::value_type`) maps to nothing
        // and is skipped, which is the same honest degradation as anywhere else (§15.4).
        let pattern = clang_getSpecializedCursorTemplate(decl);
        let source = if clang_Cursor_isNull(pattern) == 0 { pattern } else { decl };
        w.template_args = bind_template_args(source, under);
        // The class lives in a header of its own — a system one for `std::vector` — which
        // the main walk passes over. Here it is exactly what was asked for, so every
        // member it has is read against this anchor instead of its own position.
        w.anchor = Some(anchor);
        let mut fields = Vec::new();
        w.collect_fields(source, name, &mut fields);
        let (methods, move_only) = w.collect_methods(source, name);
        w.anchor = None;
        w.template_args.clear();
        // `::std::vector<float>` — the spelling the generated C++ writes, which is the
        // instantiation itself and not the Dazzle name it was given.
        let cpp = format!("::{}", spelling(under).trim_start_matches("::"));
        let kind = DeclKind::Struct {
            fields,
            opaque: false,
            is_union: clang_getCursorKind(decl) == CXCursor_UnionDecl,
            methods,
            move_only,
        };
        w.push_cpp(name.to_owned(), cpp, Vec::new(), anchor, kind);
    }
    clang_disposeTranslationUnit(tu);

    for (cpp, name) in wanted {
        if !found.contains(name) {
            w.skip(name.clone(), anchor, format!("`{cpp}` does not name a class"));
        }
    }
}

/// A constant expression's value, as clang folds it.
unsafe fn evaluate(c: CXCursor) -> Option<ConstValue> {
    let result = clang_Cursor_Evaluate(c);
    if result.is_null() {
        return None;
    }
    let kind = clang_EvalResult_getKind(result);
    let single = matches!(clang_getCanonicalType(clang_getCursorType(c)).kind, CXType_Float);
    let value = match kind {
        CXEval_Int => Some(ConstValue::Int(i128::from(clang_EvalResult_getAsLongLong(result)))),
        CXEval_Float => {
            Some(ConstValue::Float { value: clang_EvalResult_getAsDouble(result), single })
        }
        _ => None,
    };
    clang_EvalResult_dispose(result);
    value
}

/// clang's errors, formatted as clang prints them. Warnings are the header's business.
unsafe fn diagnostics(tu: CXTranslationUnit) -> Vec<String> {
    let mut out = Vec::new();
    for i in 0..clang_getNumDiagnostics(tu) {
        let d = clang_getDiagnostic(tu, i);
        if clang_getDiagnosticSeverity(d) >= CXDiagnostic_Error {
            out.push(string(clang_formatDiagnostic(d, clang_defaultDiagnosticDisplayOptions())));
        }
        clang_disposeDiagnostic(d);
    }
    out
}

/// Every file the parse read, for the cache to watch.
/// Every file the unit read, and — separately — the ones the unsaved main file includes
/// itself, which are the requested headers as the include search found them.
unsafe fn inclusions(tu: CXTranslationUnit) -> (Vec<PathBuf>, Vec<PathBuf>) {
    extern "C" fn visit(
        file: CXFile,
        _stack: *mut CXSourceLocation,
        len: c_uint,
        data: CXClientData,
    ) {
        // SAFETY: `data` is the pair passed below, alive for the whole visit.
        let out = unsafe { &mut *(data as *mut (Vec<PathBuf>, Vec<PathBuf>)) };
        let name = unsafe { string(clang_getFileName(file)) };
        if !name.is_empty() {
            // The stack holds one location per include on the way: one means the main
            // file included it directly.
            if len == 1 {
                out.1.push(PathBuf::from(&name));
            }
            out.0.push(PathBuf::from(name));
        }
    }
    let mut out: (Vec<PathBuf>, Vec<PathBuf>) = (Vec::new(), Vec::new());
    clang_getInclusions(tu, visit, &mut out as *mut (Vec<PathBuf>, Vec<PathBuf>) as CXClientData);
    // The unsaved main file is not on disk and cannot change under the cache.
    out.0.retain(|p| p.file_name().map_or(true, |n| n != "__dz_import__.cpp"));
    out
}

/// `#include` lines for `targets`, each already bracketed or quoted.
fn includes(targets: impl Iterator<Item = String>) -> String {
    targets.map(|t| format!("#include {t}\n")).collect()
}

/// A path clang can be handed from anywhere.
///
/// **Absolute**, and deliberately so: the second unit stands where the generated C++
/// will, and a quoted include is searched from *its* directory first. clang reports a
/// header by whatever path the include search found it under, which is relative whenever
/// the `-I` was — and a relative path resolved from the wrong directory finds nothing,
/// which is how the macro pass silently imported nothing for a while.
fn absolute(path: &Path) -> PathBuf {
    match std::fs::canonicalize(path) {
        // Windows' canonical form is a `\\?\` path, which clang does not take.
        Ok(p) => PathBuf::from(p.to_string_lossy().trim_start_matches(r"\\?\").to_owned()),
        Err(_) => std::env::current_dir().map(|d| d.join(path)).unwrap_or_else(|_| path.to_owned()),
    }
}

#[derive(Default)]
struct Walker {
    files: Vec<PathBuf>,
    file_index: HashMap<String, u32>,
    decls: Vec<Decl>,
    skipped: Vec<Skipped>,
    seen: HashSet<String>,
    /// Records the header defines somewhere, so a forward declaration of one of them is
    /// passed over rather than imported as a handle.
    defined: HashSet<String>,
    /// `typedef struct { … } Vector2`: an anonymous struct or enum takes the name of the
    /// typedef that names it, keyed by where it is declared.
    anon_names: HashMap<(String, u32), String>,
    /// Object-like macros whose value is not a plain number. A second parse asks clang what
    /// each one's expansion *is* — raylib's `RAYWHITE` is a `Color` (spec §15.2).
    macro_candidates: Vec<(String, Loc)>,
    /// Where a declaration read outside the header itself is recorded as being — the
    /// template instantiation pass, whose class lives in a header the main walk passes
    /// over (spec §15.4).
    anchor: Option<Loc>,
    /// The requested headers, absolute, for the passes that parse them again.
    direct: Vec<PathBuf>,
    /// A class template's parameters bound to the instantiation being imported, by name:
    /// `T` to `int` while reading `Pair<int>` (spec §15.4). Empty everywhere else.
    template_args: HashMap<String, CXType>,
    /// The comment belonging to the declaration being recorded, taken by the next push.
    pending_docs: String,
    /// The C++ namespaces being walked, outermost first (spec §15.2).
    ns: Vec<String>,
}

impl Walker {
    unsafe fn walk(&mut self, tu: CXTranslationUnit) {
        let top = flatten(children(clang_getTranslationUnitCursor(tu)));
        self.scope(tu, top);
        self.adopt_handles();
        self.drop_dangling();
    }

    /// One namespace's worth of declarations — the whole header at the outermost level, and
    /// then each `namespace` inside it, which Dazzle reaches as a nested scope (spec §15.2).
    unsafe fn scope(&mut self, tu: CXTranslationUnit, top: Vec<CXCursor>) {
        for &c in &top {
            if clang_getCursorKind(c) != CXCursor_TypedefDecl {
                continue;
            }
            let under = clang_getCanonicalType(clang_getTypedefDeclUnderlyingType(c));
            let decl = clang_getTypeDeclaration(under);
            if matches!(
                clang_getCursorKind(decl),
                CXCursor_StructDecl | CXCursor_EnumDecl | CXCursor_ClassDecl | CXCursor_UnionDecl
            ) && is_anonymous(decl)
            {
                if let Some(key) = position(decl) {
                    self.anon_names.insert(key, string(clang_getCursorSpelling(c)));
                }
            }
        }

        // Which records the header defines, so a forward declaration of one is passed over
        // rather than taken for a handle — the definition may come later in the file.
        for &c in &top {
            let record = matches!(
                clang_getCursorKind(c),
                CXCursor_StructDecl | CXCursor_UnionDecl | CXCursor_ClassDecl
            );
            if record && clang_isCursorDefinition(c) != 0 {
                let name = self.decl_name(c);
                if let Some(name) = name {
                    self.defined.insert(self.qualified(&name));
                }
            }
        }

        for c in top {
            // A namespace is walked in place: what it holds is named through it, and an
            // anonymous one has no name to reach it by, so it is left alone.
            if clang_getCursorKind(c) == CXCursor_Namespace {
                let name = string(clang_getCursorSpelling(c));
                if name.is_empty() {
                    continue;
                }
                // `namespace A { … }` written twice is one namespace, so the walk simply
                // re-enters it and the second block's declarations join the first's.
                self.ns.push(name);
                let inner = flatten(children(c));
                self.scope(tu, inner);
                self.ns.pop();
                continue;
            }
            let Some(loc) = self.place(c) else { continue };
            match clang_getCursorKind(c) {
                CXCursor_FunctionDecl => self.function(c, loc),
                CXCursor_StructDecl | CXCursor_ClassDecl => self.record(c, loc),
                CXCursor_EnumDecl => self.enumeration(c, loc),
                CXCursor_TypedefDecl | CXCursor_TypeAliasDecl => self.typedef(c, loc),
                CXCursor_MacroDefinition => self.macro_def(tu, c, loc),
                CXCursor_UnionDecl => self.record(c, loc),
                _ => {}
            }
        }
    }

    /// Where a declaration is, or `None` for what is not the header's own: a system
    /// header's, the compiler's built-ins, and the unsaved file doing the including.
    unsafe fn place(&mut self, c: CXCursor) -> Option<Loc> {
        // A template instantiation's class is somewhere the main walk would pass over — a
        // system header for `std::vector` — but it is exactly what `[cpp.instantiate]`
        // asked for, so its members are placed at the anchor instead (spec §15.4).
        if let Some(anchor) = self.anchor {
            return Some(anchor);
        }
        let loc = clang_getCursorLocation(c);
        if clang_Location_isInSystemHeader(loc) != 0 || clang_Location_isFromMainFile(loc) != 0 {
            return None;
        }
        let (file, offset) = file_location(loc)?;
        let index = match self.file_index.get(&file) {
            Some(i) => *i,
            None => {
                let i = self.files.len() as u32;
                self.files.push(PathBuf::from(&file));
                self.file_index.insert(file, i);
                i
            }
        };
        let len = string(clang_getCursorSpelling(c)).len() as u32;
        Some(Loc { file: index, start: offset, end: offset + len })
    }

    fn skip(&mut self, name: String, loc: Loc, reason: impl Into<String>) {
        self.skipped.push(Skipped { name, loc, reason: reason.into() });
    }

    /// A struct's or enum's name, or the name its typedef gives it when it has none.
    unsafe fn decl_name(&self, c: CXCursor) -> Option<String> {
        if is_anonymous(c) {
            position(c).and_then(|k| self.anon_names.get(&k).cloned())
        } else {
            Some(string(clang_getCursorSpelling(c)))
        }
    }

    fn claim(&mut self, name: &str) -> bool {
        self.seen.insert(self.qualified(name))
    }

    /// Where a declaration actually is, as C++ spells it — read from the declaration rather
    /// than from the walk, so a type referred to from elsewhere gets the same answer.
    ///
    /// This is what [`CType::Named`] carries: two namespaces may each declare a `Body`, and
    /// an unqualified name could not tell them apart (spec §15.2).
    unsafe fn cpp_path(&self, c: CXCursor) -> Option<String> {
        let name = self.decl_name(c)?;
        let mut scope = Vec::new();
        let mut parent = clang_getCursorSemanticParent(c);
        loop {
            match clang_getCursorKind(parent) {
                CXCursor_Namespace if clang_Cursor_isInlineNamespace(parent) == 0 => {
                    let n = string(clang_getCursorSpelling(parent));
                    if n.is_empty() {
                        return None; // an anonymous namespace has no name to reach it by
                    }
                    scope.push(n);
                }
                CXCursor_TranslationUnit => break,
                // An inline namespace, `extern "C"`, and a record enclosing a nested type:
                // none of them adds a segment Dazzle writes, so the walk carries on outward.
                _ => {}
            }
            let next = clang_getCursorSemanticParent(parent);
            if clang_equalCursors(next, parent) != 0 {
                break;
            }
            parent = next;
        }
        scope.reverse();
        let mut out = String::from("::");
        for n in &scope {
            out.push_str(n);
            out.push_str("::");
        }
        out.push_str(&name);
        Some(out)
    }

    /// A name as C++ spells it here: the enclosing namespaces, then the name. Leading `::`
    /// so the generated code names the global one whatever it is nested in (spec §15.2).
    fn qualified(&self, name: &str) -> String {
        let mut out = String::from("::");
        for n in &self.ns {
            out.push_str(n);
            out.push_str("::");
        }
        out.push_str(name);
        out
    }

    /// The header as a second translation unit must `#include` it.
    ///
    /// The requested headers as `#include` lines a second unit can use, by absolute path
    /// (see [`absolute`]). `None` when nothing was found to include.
    fn header_includes(&self) -> Option<String> {
        if self.direct.is_empty() {
            return None;
        }
        let quoted = self
            .direct
            .iter()
            .map(|p| format!("\"{}\"", p.display().to_string().replace('\\', "/")));
        Some(includes(quoted))
    }

    /// [`Walker::push`] for a declaration read from a cursor, whose comment travels with it.
    unsafe fn push_at(&mut self, c: CXCursor, name: String, loc: Loc, kind: DeclKind) {
        self.pending_docs = doc_comment(c);
        self.push(name, loc, kind);
        self.pending_docs.clear();
    }

    /// Record a declaration under the namespace being walked.
    fn push(&mut self, name: String, loc: Loc, kind: DeclKind) {
        // A macro is the preprocessor's, not C++'s: it has no namespace and `::WHITE` would
        // not compile, so it is emitted by the bare name it was written with (spec §15.2).
        let cpp = match kind {
            DeclKind::Const { from_macro: true, .. } => name.clone(),
            _ => self.qualified(&name),
        };
        let scope = self.ns.clone();
        self.push_cpp(name, cpp, scope, loc, kind);
    }

    /// Record a declaration whose C++ spelling is not simply its name under the current
    /// namespace: a handle adopted from a type the header never shows, or the concrete type
    /// `[cpp.instantiate]` asked for (spec §15.4).
    fn push_cpp(
        &mut self,
        name: String,
        cpp: String,
        scope: Vec<String>,
        loc: Loc,
        kind: DeclKind,
    ) {
        let docs = std::mem::take(&mut self.pending_docs);
        self.decls.push(Decl { name, scope, cpp, loc, docs, kind });
    }

    unsafe fn function(&mut self, c: CXCursor, loc: Loc) {
        let name = string(clang_getCursorSpelling(c));
        if !self.claim(&name) {
            return; // a redeclaration
        }
        if clang_isFunctionTypeVariadic(clang_getCursorType(c)) != 0 {
            return self.skip(
                name,
                loc,
                "a variadic function; spec §15.2 imports a fixed-arity one",
            );
        }
        let Some(params) = self.parameters(c, &name, loc) else { return };
        let result = clang_getCursorResultType(c);
        let ret = self.map_type(result);
        if let CType::Unsupported(what) = &ret {
            let reason = format!("it returns `{what}`");
            return self.skip(name, loc, reason);
        }
        let ret_cast = self.mismatched_spelling(result).is_some();
        self.push_at(c, name, loc, DeclKind::Fn { params, ret, ret_cast });
    }

    unsafe fn record(&mut self, c: CXCursor, loc: Loc) {
        let Some(name) = self.decl_name(c) else { return };
        // A struct the header declares and never defines is a handle it hands out —
        // `RGFW_window`, `GLFWwindow`, `FILE`. It imports as an opaque type so the
        // functions taking one keep working (spec §15.2, §18.40). A definition elsewhere
        // in the header wins, whichever came first.
        if clang_isCursorDefinition(c) == 0 {
            if !self.defined.contains(&self.qualified(&name)) && self.claim(&name) {
                let loc = Loc { end: loc.start + name.len() as u32, ..loc };
                let kind = DeclKind::Struct {
                    fields: Vec::new(),
                    opaque: true,
                    is_union: false,
                    methods: Vec::new(),
                    move_only: false,
                };
                self.push_at(c, name, loc, kind);
            }
            return;
        }
        if !self.claim(&name) {
            return;
        }
        let loc = Loc { end: loc.start + name.len() as u32, ..loc };
        let mut fields = Vec::new();
        self.collect_fields(c, &name, &mut fields);
        let is_union = clang_getCursorKind(c) == CXCursor_UnionDecl;
        let (methods, move_only) = self.collect_methods(c, &name);
        let kind = DeclKind::Struct { fields, opaque: false, is_union, methods, move_only };
        self.push_at(c, name, loc, kind);
    }

    /// A C++ class's public member functions, and whether C++ destroys it itself.
    ///
    /// Only public ones: a private member is not Dazzle's to call, and §15.2 makes a private
    /// data member opaque padding for the same reason. A method Dazzle cannot yet name — a
    /// reference parameter, an overload on a type that did not map — is recorded as skipped
    /// and the class keeps everything else (spec §15.2).
    unsafe fn collect_methods(&mut self, c: CXCursor, owner: &str) -> (Vec<Method>, bool) {
        let mut out: Vec<Method> = Vec::new();
        let (mut destructs, mut copyable) = (false, false);
        for m in children(c) {
            let kind = clang_getCursorKind(m);
            // A destructor the class writes for itself is what makes it move-only: C++ runs
            // it, so Dazzle never emits a `drop` for one (spec §15.2).
            if kind == CXCursor_Destructor {
                destructs = clang_CXXMethod_isDefaulted(m) == 0;
                continue;
            }
            if kind == CXCursor_Constructor
                && clang_CXXConstructor_isCopyConstructor(m) != 0
                && clang_getCXXAccessSpecifier(m) == CX_CXXPublic
                && clang_getCursorAvailability(m) != CXAvailability_NotAvailable
            {
                // "unless it also has a copy constructor, which makes C++ RAII types obey
                // Dazzle's rules automatically" (spec §15.2).
                copyable = true;
            }
            if !matches!(kind, CXCursor_CXXMethod | CXCursor_Constructor) {
                continue;
            }
            if clang_getCXXAccessSpecifier(m) != CX_CXXPublic {
                continue;
            }
            // A copy or move constructor is C++'s own business: Dazzle copies and moves by
            // the rules of §6.3, and calling one by hand is not how either is spelled.
            if kind == CXCursor_Constructor
                && (clang_CXXConstructor_isCopyConstructor(m) != 0
                    || clang_CXXConstructor_isMoveConstructor(m) != 0)
            {
                continue;
            }
            let Some(mloc) = self.place(m) else { continue };
            let is_ctor = kind == CXCursor_Constructor;
            let mname = if is_ctor { owner.to_owned() } else { string(clang_getCursorSpelling(m)) };
            // An operator is spelled `operator+` in C++ and is not a name Dazzle can write.
            if !is_ctor && mname.starts_with("operator") {
                continue;
            }
            let full = format!("{owner}::{mname}");
            if clang_isFunctionTypeVariadic(clang_getCursorType(m)) != 0 {
                self.skip(full, mloc, "a variadic function; spec §15.2 imports a fixed-arity one");
                continue;
            }
            let Some(params) = self.parameters(m, &full, mloc) else { continue };
            let result = clang_getCursorResultType(m);
            let ret = if is_ctor { CType::Void } else { self.map_type(result) };
            if let CType::Unsupported(what) = &ret {
                self.skip(full, mloc, format!("it returns `{what}`"));
                continue;
            }
            // A `static` member function is called through the type, and Dazzle has no
            // associated functions to call it with yet (spec §7) — so it is left out with
            // the reason, rather than imported as something nothing can name.
            if !is_ctor && clang_CXXMethod_isStatic(m) != 0 {
                self.skip(
                    full,
                    mloc,
                    "a static member function; Dazzle has no associated functions yet",
                );
                continue;
            }
            let receiver = if is_ctor {
                None
            } else if clang_CXXMethod_isConst(m) != 0 {
                Some(Receiver::Ref)
            } else {
                Some(Receiver::MutRef)
            };
            out.push(Method {
                name: mname,
                loc: mloc,
                params,
                ret_cast: !is_ctor && self.mismatched_spelling(result).is_some(),
                ret,
                receiver,
                docs: doc_comment(m),
                is_ctor,
            });
        }
        (out, destructs && !copyable)
    }

    /// One function's parameters, or `None` when any of them is a type Dazzle cannot name —
    /// which takes the whole function out of the import, with the reason recorded.
    unsafe fn parameters(&mut self, c: CXCursor, what: &str, loc: Loc) -> Option<Vec<Param>> {
        let mut params = Vec::new();
        for i in 0..clang_Cursor_getNumArguments(c).max(0) as c_uint {
            let p = clang_Cursor_getArgument(c, i);
            let mut pname = string(clang_getCursorSpelling(p));
            if pname.is_empty() {
                pname = format!("arg{i}");
            }
            let param_ty = clang_getCursorType(p);
            let ty = self.map_type(param_ty);
            if let CType::Unsupported(w) = &ty {
                let reason = format!("its parameter `{pname}` is `{w}`");
                self.skip(what.to_owned(), loc, reason);
                return None;
            }
            params.push(Param { name: pname, ty, c_cast: self.mismatched_spelling(param_ty) });
        }
        Some(params)
    }

    /// One record's members. An anonymous `union { … };` or `struct { … };` written inside
    /// it has no name of its own, and C makes what it holds a member of the enclosing
    /// struct — `event.key` — so its members are collected here too (spec §15.2).
    unsafe fn collect_fields(&mut self, c: CXCursor, owner: &str, out: &mut Vec<Field>) {
        for f in children(c) {
            match clang_getCursorKind(f) {
                CXCursor_FieldDecl => {}
                CXCursor_StructDecl | CXCursor_UnionDecl
                    if clang_Cursor_isAnonymousRecordDecl(f) != 0 =>
                {
                    self.collect_fields(f, owner, out);
                    continue;
                }
                _ => continue,
            }
            // A private or protected C++ member is not Dazzle's to name. It stays in the
            // type — the layout is the header's, since Dazzle never redeclares it — so it
            // is opaque padding exactly as §15.2 says. A C struct's members come through
            // this as public, which is what they are.
            if matches!(clang_getCXXAccessSpecifier(f), CX_CXXPrivate | CX_CXXProtected) {
                continue;
            }
            let fname = string(clang_getCursorSpelling(f));
            let Some(floc) = self.place(f) else { continue };
            // A field Dazzle cannot name is left out, not the struct: it is still there in
            // C++, zeroed with the rest, so the layout stays C's own (spec §15.2).
            let bitfield = clang_Cursor_isBitField(f) != 0;
            match self.map_type(clang_getCursorType(f)) {
                CType::Unsupported(what) => {
                    self.skip(format!("{owner}.{fname}"), floc, format!("its type is `{what}`"))
                }
                ty => {
                    out.push(Field { name: fname, ty, loc: floc, docs: doc_comment(f), bitfield })
                }
            }
        }
    }

    unsafe fn enumeration(&mut self, c: CXCursor, loc: Loc) {
        if clang_isCursorDefinition(c) == 0 {
            return;
        }
        let Some(name) = self.decl_name(c) else { return };
        if !self.claim(&name) {
            return;
        }
        let loc = Loc { end: loc.start + name.len() as u32, ..loc };
        let backing = match self.map_type(clang_getEnumDeclIntegerType(c)) {
            CType::Int(i) => i,
            _ => CInt { bits: 32, signed: true },
        };
        let scoped = clang_EnumDecl_isScoped(c) != 0;
        let mut variants = Vec::new();
        for v in children(c) {
            if clang_getCursorKind(v) != CXCursor_EnumConstantDecl {
                continue;
            }
            let Some(vloc) = self.place(v) else { continue };
            let value = if backing.signed {
                i128::from(clang_getEnumConstantDeclValue(v))
            } else {
                i128::from(clang_getEnumConstantDeclUnsignedValue(v))
            };
            variants.push(Enumerator {
                name: string(clang_getCursorSpelling(v)),
                value,
                loc: vloc,
            });
        }
        // C code uses an unscoped enum's enumerators as plain integers — raylib's
        // `IsKeyDown(int key)` takes a `KEY_A` — so each is also a constant of its own
        // (spec §15.2). An `enum class` keeps them behind its name.
        if !scoped {
            for v in &variants {
                if self.claim(&v.name) {
                    let kind =
                        DeclKind::Const { value: ConstValue::Int(v.value), from_macro: false };
                    self.push(v.name.clone(), v.loc, kind);
                }
            }
        }
        self.push_at(c, name, loc, DeclKind::Enum { scoped, backing, variants });
    }

    /// A typedef naming a struct or enum under a second name becomes an alias. One naming
    /// anything else needs no symbol: every use of it is mapped through to the type itself.
    unsafe fn typedef(&mut self, c: CXCursor, loc: Loc) {
        let name = string(clang_getCursorSpelling(c));
        let under = clang_getCanonicalType(clang_getTypedefDeclUnderlyingType(c));
        let decl = clang_getTypeDeclaration(under);
        if !matches!(
            clang_getCursorKind(decl),
            CXCursor_StructDecl | CXCursor_EnumDecl | CXCursor_ClassDecl | CXCursor_UnionDecl
        ) {
            return;
        }
        let Some(target) = self.cpp_path(decl) else { return };
        if target == self.qualified(&name) || !self.claim(&name) {
            return;
        }
        self.push_at(c, name, loc, DeclKind::Alias { target });
    }

    unsafe fn macro_def(&mut self, tu: CXTranslationUnit, c: CXCursor, loc: Loc) {
        if clang_Cursor_isMacroFunctionLike(c) != 0 {
            return;
        }
        let name = string(clang_getCursorSpelling(c));
        let tokens = tokens(tu, clang_getCursorExtent(c));
        // The first token is the macro's own name; an empty body is an include guard.
        let body: Vec<&str> = tokens.iter().skip(1).map(String::as_str).collect();
        if body.is_empty() {
            return;
        }
        match number(&body) {
            Some(value) if self.claim(&name) => {
                self.push_at(c, name, loc, DeclKind::Const { value, from_macro: true })
            }
            Some(_) => {}
            // Not a number: a second parse decides whether it is anything else Dazzle can
            // name, and records the skip if it is not.
            None => self.macro_candidates.push((name, loc)),
        }
    }

    /// The C spelling of a pointer whose pointee Dazzle spells as a different C++ type:
    /// `char *` against `int8_t *`, `long *` against `int32_t *`. Only these need a cast at
    /// the boundary; `int *`, `float *` and a struct's are already the same type (§15.2).
    unsafe fn mismatched_spelling(&self, t: CXType) -> Option<String> {
        if t.kind != CXType_Pointer {
            return None;
        }
        let pointee = clang_getPointeeType(t);
        // `const char *` is `cstr`, which is that type already.
        if clang_isConstQualifiedType(pointee) != 0
            && matches!(clang_getCanonicalType(pointee).kind, CXType_Char_S | CXType_Char_U)
        {
            return None;
        }
        let mismatched = matches!(
            clang_getCanonicalType(pointee).kind,
            CXType_Char_S
                | CXType_Char_U
                | CXType_Long
                | CXType_ULong
                | CXType_LongLong
                | CXType_ULongLong
        );
        mismatched.then(|| spelling(t))
    }

    /// `signature` is the function type a pointer points at; `whole` is the pointer, whose
    /// spelling names the type if any part of it cannot be mapped.
    unsafe fn function_pointer(&self, signature: CXType, whole: CXType) -> CType {
        if clang_isFunctionTypeVariadic(signature) != 0 {
            return CType::Unsupported(spelling(whole));
        }
        let mut params = Vec::new();
        for i in 0..clang_getNumArgTypes(signature).max(0) as c_uint {
            match self.map_type(clang_getArgType(signature, i)) {
                CType::Unsupported(_) => return CType::Unsupported(spelling(whole)),
                ty => params.push(ty),
            }
        }
        match self.map_type(clang_getResultType(signature)) {
            CType::Unsupported(_) => CType::Unsupported(spelling(whole)),
            ret => CType::Fn { params, ret: Box::new(ret) },
        }
    }

    unsafe fn map_type(&self, t: CXType) -> CType {
        // Inside a template being instantiated, a parameter is whatever it was bound to:
        // `T` reached through `Pair<int>` is `int` (spec §15.4). Pointers and arrays get
        // here by recursion, so binding the leaf is enough for `T`, `T*` and `T[4]` alike.
        if !self.template_args.is_empty() {
            if let Some(arg) = self.template_args.get(&spelling(t)) {
                return self.map_type(*arg);
            }
        }
        match t.kind {
            CXType_Elaborated => return self.map_type(clang_Type_getNamedType(t)),
            CXType_Typedef => {
                let name = string(clang_getCursorSpelling(clang_getTypeDeclaration(t)));
                match name.as_str() {
                    "size_t" | "uintptr_t" => return CType::Size { signed: false },
                    "ptrdiff_t" | "intptr_t" | "ssize_t" => return CType::Size { signed: true },
                    _ => return self.map_type(clang_getCanonicalType(t)),
                }
            }
            _ => {}
        }
        let int =
            |signed: bool| CType::Int(CInt { bits: (clang_Type_getSizeOf(t) * 8) as u8, signed });
        match t.kind {
            CXType_Void => CType::Void,
            CXType_Bool => CType::Bool,
            CXType_Char_S | CXType_SChar | CXType_Short | CXType_Int | CXType_Long
            | CXType_LongLong => int(true),
            CXType_Char_U | CXType_UChar | CXType_UShort | CXType_UInt | CXType_ULong
            | CXType_ULongLong => int(false),
            CXType_Float => CType::Float { bits: 32 },
            CXType_Double => CType::Float { bits: 64 },
            CXType_Pointer => {
                let pointee = clang_getPointeeType(t);
                let mutable = clang_isConstQualifiedType(pointee) == 0;
                let canonical = clang_getCanonicalType(pointee);
                match canonical.kind {
                    CXType_Char_S | CXType_Char_U if !mutable => CType::CStr,
                    // `char *` and `long *`: Dazzle's `^mut i8` and `^mut i32` are the right
                    // types, but `int8_t` is `signed char` and `int32_t` is `int`, which are
                    // distinct C++ types that pointers do not convert between. The mapping
                    // stands and the crossing is cast — see `mismatched_spelling` (§15.2).
                    CXType_WChar => CType::Unsupported(spelling(t)),
                    // A callback: the pointer is the type, as it is in C (spec §15.2).
                    CXType_FunctionProto | CXType_FunctionNoProto => {
                        self.function_pointer(canonical, t)
                    }
                    _ => match self.map_type(pointee) {
                        CType::Unsupported(_) => CType::Unsupported(spelling(t)),
                        inner => CType::Ptr { mutable, pointee: Box::new(inner) },
                    },
                }
            }
            // `const Vector3&` is a `&Vector3` and `Vector3&` a `&mut Vector3` (§15.2).
            // An rvalue reference is not a Dazzle type: `T&&` is C++'s way of spelling a
            // move, and §6.4 spells a move its own way, so it is left out.
            CXType_LValueReference => {
                let referent = clang_getPointeeType(t);
                let mutable = clang_isConstQualifiedType(referent) == 0;
                match self.map_type(referent) {
                    CType::Unsupported(_) => CType::Unsupported(spelling(t)),
                    inner => CType::Ref { mutable, referent: Box::new(inner) },
                }
            }
            // `float m[16]` in a struct. A parameter written as an array is a pointer in C
            // and arrives here as one, so this is the member case (spec §15.2).
            CXType_ConstantArray => {
                let len = clang_getArraySize(t);
                let elem = self.map_type(clang_getArrayElementType(t));
                match (u64::try_from(len), elem) {
                    (Ok(len), elem) if !matches!(elem, CType::Unsupported(_)) => {
                        CType::Array { len, elem: Box::new(elem) }
                    }
                    _ => CType::Unsupported(spelling(t)),
                }
            }
            CXType_Record | CXType_Enum => {
                let decl = clang_getTypeDeclaration(t);
                match self.cpp_path(decl) {
                    Some(n) => CType::Named(n),
                    None => CType::Unsupported(spelling(t)),
                }
            }
            _ => CType::Unsupported(spelling(t)),
        }
    }

    /// A type the header never shows — declared in a system header it includes, or nowhere
    /// at all — is a handle when every use of it is behind a pointer: `FILE *`, `HWND`.
    /// Imported as opaque, so the functions taking one keep working. Used by value its size
    /// is genuinely unknown, and `drop_dangling` takes those declarations away instead.
    fn adopt_handles(&mut self) {
        let known: HashSet<String> = self
            .decls
            .iter()
            .filter(|d| {
                matches!(
                    d.kind,
                    DeclKind::Struct { .. } | DeclKind::Enum { .. } | DeclKind::Alias { .. }
                )
            })
            .map(|d| d.cpp.clone())
            .collect();

        // Where each unknown name is used, and the declaration that first mentions it, so
        // the handle has somewhere in the header to point at.
        let (mut behind_pointer, mut by_value) = (Vec::new(), HashSet::new());
        fn scan(
            t: &CType,
            pointer: bool,
            at: Loc,
            known: &HashSet<String>,
            behind: &mut Vec<(String, Loc)>,
            value: &mut HashSet<String>,
        ) {
            match t {
                CType::Ptr { pointee, .. } => scan(pointee, true, at, known, behind, value),
                // A reference is a pointer for this purpose: a type only ever reached
                // through one is still a handle, never a value (spec §18.40).
                CType::Ref { referent, .. } => scan(referent, true, at, known, behind, value),
                CType::Array { elem, .. } => scan(elem, pointer, at, known, behind, value),
                // A callback's own parameters are values of their types, whatever the
                // pointer to the function is.
                CType::Fn { params, ret } => {
                    for p in params.iter().chain(std::iter::once(&**ret)) {
                        scan(p, false, at, known, behind, value);
                    }
                }
                CType::Named(n) if !known.contains(n) => {
                    if pointer {
                        behind.push((n.clone(), at));
                    } else {
                        value.insert(n.clone());
                    }
                }
                _ => {}
            }
        }
        let walk =
            |t: &CType, at: Loc, behind: &mut Vec<(String, Loc)>, value: &mut HashSet<String>| {
                scan(t, false, at, &known, behind, value);
            };
        for d in &self.decls {
            match &d.kind {
                DeclKind::Fn { params, ret, .. } => {
                    for p in params {
                        walk(&p.ty, d.loc, &mut behind_pointer, &mut by_value);
                    }
                    walk(ret, d.loc, &mut behind_pointer, &mut by_value);
                }
                DeclKind::Struct { fields, .. } => {
                    for f in fields {
                        walk(&f.ty, f.loc, &mut behind_pointer, &mut by_value);
                    }
                }
                _ => {}
            }
        }

        for (cpp, loc) in behind_pointer {
            if by_value.contains(&cpp) || !self.seen.insert(cpp.clone()) {
                continue;
            }
            // The path is already qualified — it came from the type, not from the walk.
            let (scope, name) = split_path(&cpp);
            let kind = DeclKind::Struct {
                fields: Vec::new(),
                opaque: true,
                is_union: false,
                methods: Vec::new(),
                move_only: false,
            };
            self.push_cpp(name, cpp, scope, loc, kind);
        }
    }

    /// A declaration naming a struct or enum that was not imported cannot be either:
    /// a function goes, a field is hidden. Repeated until nothing changes, since dropping
    /// one can strand another.
    fn drop_dangling(&mut self) {
        loop {
            let named: HashSet<String> = self
                .decls
                .iter()
                .filter(|d| {
                    matches!(
                        d.kind,
                        DeclKind::Struct { .. } | DeclKind::Enum { .. } | DeclKind::Alias { .. }
                    )
                })
                .map(|d| d.cpp.clone())
                .collect();
            fn first_missing(t: &CType, named: &HashSet<String>) -> Option<String> {
                match t {
                    CType::Ptr { pointee, .. } => first_missing(pointee, named),
                    CType::Ref { referent, .. } => first_missing(referent, named),
                    CType::Array { elem, .. } => first_missing(elem, named),
                    CType::Fn { params, ret } => params
                        .iter()
                        .chain(std::iter::once(&**ret))
                        .find_map(|p| first_missing(p, named)),
                    CType::Named(x) if !named.contains(x) => Some(x.clone()),
                    _ => None,
                }
            }
            let missing = |t: &CType| first_missing(t, &named);
            let mut changed = false;
            let mut skipped = Vec::new();
            self.decls.retain_mut(|d| match &mut d.kind {
                DeclKind::Fn { params, ret, .. } => {
                    let gone = params
                        .iter()
                        .map(|p| &p.ty)
                        .chain(std::iter::once(&*ret))
                        .find_map(missing);
                    match gone {
                        Some(x) => {
                            skipped.push(Skipped {
                                name: d.name.clone(),
                                loc: d.loc,
                                reason: format!("it uses `{x}`, which was not imported"),
                            });
                            changed = true;
                            false
                        }
                        None => true,
                    }
                }
                DeclKind::Struct { fields, .. } => {
                    let before = fields.len();
                    fields.retain(|f| missing(&f.ty).is_none());
                    changed |= fields.len() != before;
                    true
                }
                DeclKind::Alias { target } if !named.contains(target.as_str()) => {
                    changed = true;
                    false
                }
                _ => true,
            });
            self.skipped.extend(skipped);
            if !changed {
                return;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// helpers over the C API
// ---------------------------------------------------------------------------

fn cstring(s: &str) -> CString {
    CString::new(s.replace('\0', "")).unwrap_or_default()
}

unsafe fn string(s: CXString) -> String {
    let p = clang_getCString(s);
    let out =
        if p.is_null() { String::new() } else { CStr::from_ptr(p).to_string_lossy().into_owned() };
    clang_disposeString(s);
    out
}

unsafe fn spelling(t: CXType) -> String {
    string(clang_getTypeSpelling(t))
}

/// The documentation comment above a declaration, with its markers taken off.
///
/// Every C and C++ comment style a header might use: `/** … */` and `/*! … */` with or
/// without leading `*` on each line, and runs of `///` or `//!`. What is left — Doxygen's
/// `@param`, `\brief` and the rest — is kept exactly as the header's author wrote it, since
/// it is their words that are worth showing and any rewriting would lose some of them.
unsafe fn doc_comment(c: CXCursor) -> String {
    let raw = string(clang_Cursor_getRawCommentText(c));
    if raw.is_empty() {
        return String::new();
    }
    let mut lines: Vec<String> = Vec::new();
    let block = raw.trim_start().starts_with("/*");
    if block {
        let inner = raw
            .trim()
            .trim_start_matches("/**")
            .trim_start_matches("/*!")
            .trim_start_matches("/*")
            .trim_end_matches("*/");
        for line in inner.lines() {
            let l = line.trim_start();
            lines.push(l.strip_prefix('*').unwrap_or(l).trim_end().to_owned());
        }
    } else {
        for line in raw.lines() {
            let l = line.trim_start();
            let l = l
                .strip_prefix("///")
                .or_else(|| l.strip_prefix("//!"))
                .unwrap_or_else(|| l.strip_prefix("//").unwrap_or(l));
            lines.push(l.trim_end().to_owned());
        }
    }
    // Headers indent the body of a block comment; a common indent is decoration, not
    // meaning, so it goes — but the relative indent of an example stays.
    let indent = lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);
    let text: Vec<&str> =
        lines.iter().map(|l| if l.len() >= indent { &l[indent..] } else { "" }).collect();
    text.join("\n").trim().to_owned()
}

/// `extern "C" { … }` is how a C header speaks to C++, and an inline namespace is a
/// namespace whose contents are also named through its parent: what either holds belongs to
/// the scope around it, so both are opened out in place (spec §15.2).
unsafe fn flatten(cs: Vec<CXCursor>) -> Vec<CXCursor> {
    let mut out = Vec::with_capacity(cs.len());
    for c in cs {
        let inline_ns =
            clang_getCursorKind(c) == CXCursor_Namespace && clang_Cursor_isInlineNamespace(c) != 0;
        if clang_getCursorKind(c) == CXCursor_LinkageSpec || inline_ns {
            out.extend(flatten(children(c)));
        } else {
            out.push(c);
        }
    }
    out
}

/// A class template's parameters bound to one instantiation's arguments: `T` to `int` for
/// `Pair<int>` (spec §15.4).
///
/// By name, because that is what a member's type is written as in the template's own AST —
/// `map_type` looks a leaf up here before deciding it cannot name it.
unsafe fn bind_template_args(pattern: CXCursor, instance: CXType) -> HashMap<String, CXType> {
    let mut out = HashMap::new();
    let params: Vec<CXCursor> = children(pattern)
        .into_iter()
        .filter(|c| clang_getCursorKind(*c) == CXCursor_TemplateTypeParameter)
        .collect();
    for (i, p) in params.iter().enumerate() {
        let arg = clang_Type_getTemplateArgumentAsType(instance, i as c_uint);
        if arg.kind != CXType_Invalid {
            out.insert(string(clang_getCursorSpelling(*p)), arg);
        }
    }
    out
}

/// `::JPH::Body` into the namespaces around it and the name Dazzle knows it by.
fn split_path(cpp: &str) -> (Vec<String>, String) {
    let mut parts: Vec<String> =
        cpp.trim_start_matches("::").split("::").map(str::to_owned).collect();
    let name = parts.pop().unwrap_or_default();
    (parts, name)
}

unsafe fn children(c: CXCursor) -> Vec<CXCursor> {
    extern "C" fn visit(c: CXCursor, _parent: CXCursor, data: CXClientData) -> CXChildVisitResult {
        // SAFETY: `data` is the `Vec` passed below, alive for the whole visit.
        unsafe { (*(data as *mut Vec<CXCursor>)).push(c) };
        CXChildVisit_Continue
    }
    let mut out: Vec<CXCursor> = Vec::new();
    clang_visitChildren(c, visit, &mut out as *mut Vec<CXCursor> as CXClientData);
    out
}

unsafe fn file_location(loc: CXSourceLocation) -> Option<(String, u32)> {
    let mut file: CXFile = std::ptr::null_mut();
    let (mut line, mut column, mut offset) = (0, 0, 0);
    clang_getFileLocation(loc, &mut file, &mut line, &mut column, &mut offset);
    if file.is_null() {
        return None;
    }
    Some((string(clang_getFileName(file)), offset))
}

unsafe fn position(c: CXCursor) -> Option<(String, u32)> {
    file_location(clang_getCursorLocation(c))
}

/// Newer clangs spell an anonymous declaration `(unnamed struct at …)`, older ones not at
/// all.
unsafe fn is_anonymous(c: CXCursor) -> bool {
    let name = string(clang_getCursorSpelling(c));
    name.is_empty() || name.contains(' ') || clang_Cursor_isAnonymous(c) != 0
}

unsafe fn tokens(tu: CXTranslationUnit, range: CXSourceRange) -> Vec<String> {
    let mut toks: *mut CXToken = std::ptr::null_mut();
    let mut n: c_uint = 0;
    clang_tokenize(tu, range, &mut toks, &mut n);
    let mut out = Vec::with_capacity(n as usize);
    for i in 0..n as usize {
        out.push(string(clang_getTokenSpelling(tu, *toks.add(i))));
    }
    if !toks.is_null() {
        clang_disposeTokens(tu, toks, n);
    }
    out
}

/// The value of a macro body that is a number: `7`, `0x10u`, `0.5f`, `-1`, `(1 << 0)` is
/// not one. Parentheses around the whole are allowed, since macros are often written so.
fn number(body: &[&str]) -> Option<ConstValue> {
    let mut toks = body;
    while toks.len() >= 2 && toks[0] == "(" && toks[toks.len() - 1] == ")" {
        toks = &toks[1..toks.len() - 1];
    }
    let (negative, text) = match toks {
        ["-", t] => (true, *t),
        [t] => (false, *t),
        _ => return None,
    };
    let lower = text.to_ascii_lowercase().replace('\'', "");
    let hex = lower.starts_with("0x");
    if !hex && (lower.contains('.') || lower.contains('e')) {
        let single = lower.ends_with('f');
        let v: f64 = lower.trim_end_matches(['f', 'l']).parse().ok()?;
        return Some(ConstValue::Float { value: if negative { -v } else { v }, single });
    }
    let digits = lower.trim_end_matches(['u', 'l']);
    let v = if let Some(h) = digits.strip_prefix("0x") {
        i128::from_str_radix(h, 16).ok()?
    } else if let Some(b) = digits.strip_prefix("0b") {
        i128::from_str_radix(b, 2).ok()?
    } else if digits.len() > 1 && digits.starts_with('0') {
        i128::from_str_radix(&digits[1..], 8).ok()?
    } else {
        digits.parse().ok()?
    };
    Some(ConstValue::Int(if negative { -v } else { v }))
}

#[cfg(test)]
mod tests {
    use super::{number, ConstValue};

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
