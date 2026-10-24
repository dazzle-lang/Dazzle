//! The headers a WebAssembly build of the compiler carries, since it has no libclang.
//!
//! One: the C library as the standard library imports it, and only what `std` calls from
//! it (spec §14.2). That is enough to check and generate any program that does not import a
//! header of its own; one that does is told this build cannot read it. The generated C++ is
//! the same either way — it includes the real `<stdlib.h>` and calls `::malloc` — so only
//! what the compiler *knows* about the header is abridged, never what it writes.
//!
//! A test holds these declarations to what libclang reads from the real header, so the two
//! builds cannot drift apart.

use std::path::Path;
#[cfg(target_family = "wasm")]
use std::sync::{Arc, OnceLock};

use crate::*;

/// Where the carried header is shown: it names nothing on disk.
const STDLIB_PATH: &str = "<builtin>/stdlib.h";

const STDLIB_H: &str = "\
// The part of the C library's `stdlib.h` the standard library calls (spec §14.2), as
// carried by a build of the compiler that cannot read the real one.
void* malloc(size_t size);
void free(void* ptr);
";

/// The carried header's text, when `path` is its path.
#[cfg_attr(not(target_family = "wasm"), allow(dead_code))]
pub(crate) fn text(path: &Path) -> Option<&'static str> {
    (path == Path::new(STDLIB_PATH)).then_some(STDLIB_H)
}

/// `stdlib.h` as it would be imported, abridged to what `std` calls.
#[cfg_attr(not(target_family = "wasm"), allow(dead_code))]
pub(crate) fn stdlib() -> Header {
    let loc = |name: &str| {
        let start = STDLIB_H.find(&format!("{name}(")).unwrap_or(0) as u32;
        Loc { file: 0, start, end: start + name.len() as u32 }
    };
    let void_ptr = CType::Ptr { mutable: true, pointee: Box::new(CType::Void) };
    let func = |name: &str, params: Vec<Param>, ret: CType| Decl {
        name: name.to_owned(),
        scope: Vec::new(),
        cpp: format!("::{name}"),
        loc: loc(name),
        docs: String::new(),
        kind: DeclKind::Fn { params, ret, ret_cast: false },
    };
    let param = |name: &str, ty: CType| Param { name: name.to_owned(), ty, c_cast: None };
    Header {
        requested: vec!["stdlib.h".to_owned()],
        paths: vec![STDLIB_PATH.into()],
        path: STDLIB_PATH.into(),
        files: vec![STDLIB_PATH.into()],
        decls: vec![
            func("malloc", vec![param("size", CType::Size { signed: false })], void_ptr.clone()),
            func("free", vec![param("ptr", void_ptr)], CType::Void),
        ],
        skipped: Vec::new(),
    }
}

/// Import one header: the carried one, or an explanation of why no other can be.
#[cfg(target_family = "wasm")]
pub fn import(req: &Request<'_>) -> Result<Arc<Header>, ImportError> {
    // One `Arc` for the process, as the libclang importer's cache gives: the driver reuses a
    // checked standard library only for the very headers it was checked against.
    static STDLIB: OnceLock<Arc<Header>> = OnceLock::new();
    if req.headers == ["stdlib.h"] {
        return Ok(STDLIB.get_or_init(|| Arc::new(stdlib())).clone());
    }
    Err(ImportError::Unavailable(format!(
        "this build of the compiler runs as WebAssembly, where there is no libclang to read \
         `{}` with",
        req.headers.join("`, `")
    )))
}
