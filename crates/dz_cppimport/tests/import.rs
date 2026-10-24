//! The importer against a real header: `tests/run/cinterop`'s `mini.h` (spec §15.2).

use std::path::{Path, PathBuf};

use dz_cppimport::{import, CInt, CType, ConstValue, DeclKind, ImportError, Request};

fn mini() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/run/cinterop/vendor/mini/include")
}

/// `None` when this machine has no libclang, which is reported rather than failed: the
/// rest of the compiler does not need it.
fn import_mini() -> Option<std::sync::Arc<dz_cppimport::Header>> {
    let dirs = [mini()];
    let req = Request {
        headers: &["mini.h".to_owned()],
        include_dirs: &dirs,
        defines: &[],
        cpp_standard: "c++17",
        from_dir: &std::env::temp_dir(),
        instantiate: &[],
    };
    match import(&req) {
        Ok(h) => Some(h),
        Err(ImportError::Unavailable(e)) => {
            eprintln!("skipping: libclang is not available: {e}");
            None
        }
        Err(e) => panic!("mini.h did not import: {e:?}"),
    }
}

const I32: CType = CType::Int(CInt { bits: 32, signed: true });

#[test]
fn a_c_header_imports_its_functions_structs_enums_and_macros() {
    let Some(h) = import_mini() else { return };
    let kind = |name: &str| {
        &h.decl(name).unwrap_or_else(|| panic!("`{name}` was not imported: {h:#?}")).kind
    };

    assert!(matches!(kind("MiniVec2"), DeclKind::Struct { fields, .. } if fields.len() == 2
        && fields[0].name == "x" && fields[0].ty == CType::Float { bits: 32 }));
    match kind("mini_add") {
        DeclKind::Fn { params, ret, .. } => {
            assert_eq!(params.len(), 2);
            assert_eq!(params[0].ty, CType::Named("::MiniVec2".into()));
            assert_eq!(*ret, CType::Named("::MiniVec2".into()));
        }
        other => panic!("{other:?}"),
    }
    // `int *` is writable and `const int *` is not (spec §15.2, §18.37).
    match kind("mini_fill") {
        DeclKind::Fn { params, .. } => {
            assert_eq!(params[0].ty, CType::Ptr { mutable: true, pointee: Box::new(I32) })
        }
        other => panic!("{other:?}"),
    }
    match kind("mini_sum") {
        DeclKind::Fn { params, .. } => {
            assert_eq!(params[0].ty, CType::Ptr { mutable: false, pointee: Box::new(I32) })
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(kind("mini_name"), DeclKind::Fn { ret: CType::CStr, .. }));

    // An unscoped enum is a type, and each enumerator a constant of its own.
    assert!(
        matches!(kind("MiniShape"), DeclKind::Enum { scoped: false, variants, .. } if variants.len() == 2)
    );
    assert!(matches!(
        kind("MINI_SQUARE"),
        DeclKind::Const { value: ConstValue::Int(4), from_macro: false }
    ));

    assert!(matches!(
        kind("MINI_VERSION"),
        DeclKind::Const { value: ConstValue::Int(7), from_macro: true }
    ));
    assert!(matches!(
        kind("MINI_HALF"),
        DeclKind::Const { value: ConstValue::Float { single: true, .. }, .. }
    ));
    // The include guard is not a constant.
    assert!(h.decl("MINI_H").is_none());
}

#[test]
fn a_struct_the_header_never_defines_is_a_handle() {
    // spec §15.2, §18.40 — `MiniCounter` is declared and defined only in the library's own
    // source, as RGFW's window and C's `FILE` are.
    let Some(h) = import_mini() else { return };
    let kind =
        |name: &str| &h.decl(name).unwrap_or_else(|| panic!("`{name}` was not imported")).kind;

    assert!(
        matches!(kind("MiniCounter"), DeclKind::Struct { opaque: true, fields, .. } if fields.is_empty())
    );
    // The functions that pass one around import as usual.
    let handle = CType::Named("::MiniCounter".into());
    match kind("mini_bump") {
        DeclKind::Fn { params, .. } => {
            assert_eq!(
                params[0].ty,
                CType::Ptr { mutable: true, pointee: Box::new(handle.clone()) }
            )
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        kind("mini_counter"),
        DeclKind::Fn { ret: CType::Ptr { mutable: true, .. }, .. }
    ));
    match kind("mini_count") {
        DeclKind::Fn { params, .. } => {
            assert_eq!(params[0].ty, CType::Ptr { mutable: false, pointee: Box::new(handle) })
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_c_union_imports_as_a_type_whose_members_overlap() {
    // spec §15.2, §18.41 — Dazzle's own `union` is tagged and a C one is not, so it comes
    // in as a type with members, the way RGFW's event does.
    let Some(h) = import_mini() else { return };
    let kind =
        |name: &str| &h.decl(name).unwrap_or_else(|| panic!("`{name}` was not imported")).kind;

    match kind("MiniValue") {
        DeclKind::Struct { fields, is_union: true, opaque: false, .. } => {
            assert_eq!(fields.len(), 2);
            assert_eq!(fields[0].name, "as_int");
            assert_eq!(fields[0].ty, I32);
            assert_eq!(fields[1].ty, CType::Float { bits: 32 });
        }
        other => panic!("{other:?}"),
    }
    // And the functions that take or return one import as usual.
    assert!(
        matches!(kind("mini_int_value"), DeclKind::Fn { ret, .. } if *ret == CType::Named("::MiniValue".into()))
    );
    assert!(matches!(kind("mini_read_int"), DeclKind::Fn { params, .. } if params[0].ty
        == CType::Ptr { mutable: false, pointee: Box::new(CType::Named("::MiniValue".into())) }));
}

#[test]
fn a_field_of_an_unnamed_struct_names_it_through_decltype() {
    // spec §15.2 — miniaudio's `ma_device_config` keeps its playback settings in
    // `struct { … } playback;`. Leaving the field out would leave no way to set them.
    let Some(h) = import_mini() else { return };
    let decl = |name: &str| h.decl(name).unwrap_or_else(|| panic!("`{name}` was not imported"));

    let output = CType::Named("decltype(::MiniConfig::output)".into());
    match &decl("MiniConfig").kind {
        DeclKind::Struct { fields, .. } => {
            assert_eq!(fields.len(), 2, "{fields:?}");
            assert_eq!(fields[1].name, "output");
            assert_eq!(fields[1].ty, output);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(decl("MiniConfig_output").cpp, "decltype(::MiniConfig::output)");
    // One level down, through the first.
    let gain = decl("MiniConfig_output_gain");
    assert_eq!(gain.cpp, "decltype(decltype(::MiniConfig::output)::gain)");
    assert!(matches!(&gain.kind, DeclKind::Struct { fields, .. } if fields.len() == 2));
}

#[test]
fn a_struct_keeps_its_array_members_and_bit_fields() {
    // spec §15.2 — the shapes raylib's `Matrix` and RGFW's device structs are built from.
    let Some(h) = import_mini() else { return };
    match &h.decl("MiniFlags").expect("MiniFlags was not imported").kind {
        DeclKind::Struct { fields, .. } => {
            assert_eq!(fields.len(), 3, "{fields:?}");
            assert!(fields[0].bitfield && fields[1].bitfield, "{fields:?}");
            assert_eq!(fields[2].name, "samples");
            assert_eq!(fields[2].ty, CType::Array { len: 4, elem: Box::new(I32) });
            assert!(!fields[2].bitfield);
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_callback_imports_as_a_function_type() {
    // spec §15.2 — a C function pointer is Dazzle's own `fn(…) -> T`, which is how RGFW's
    // `void (*)(const RGFW_event *)` callbacks are handed a Dazzle function.
    let Some(h) = import_mini() else { return };
    match &h.decl("mini_apply").expect("mini_apply was not imported").kind {
        DeclKind::Fn { params, ret, .. } => {
            assert_eq!(*ret, I32);
            assert_eq!(params[0].ty, CType::Fn { params: vec![I32], ret: Box::new(I32) });
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_declaration_points_at_its_name_in_the_header() {
    let Some(h) = import_mini() else { return };
    let d = h.decl("mini_add").unwrap();
    let text = std::fs::read_to_string(&h.files[d.loc.file as usize]).unwrap();
    assert_eq!(&text[d.loc.start as usize..d.loc.end as usize], "mini_add");
}

/// A library whose API is spread over several headers is one import (spec §15.1): what each
/// declares is imported, each header's path is where the search found it, and the second
/// passes — a macro's type, here — see every header rather than only the first.
#[test]
fn several_headers_are_one_import() {
    let dir = std::env::temp_dir().join("dz_import_two_headers");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("first.h"), "#pragma once\ntypedef struct Pair { int a; int b; } Pair;\nextern \"C\" int first(int x);\n").unwrap();
    std::fs::write(
        dir.join("second.h"),
        "#pragma once\n#include \"first.h\"\nextern \"C\" Pair second(Pair p);\n#define ORIGIN ((Pair){ 0, 0 })\n",
    )
    .unwrap();
    let dirs = [dir.clone()];
    let headers = ["first.h".to_owned(), "second.h".to_owned()];
    let req = Request {
        headers: &headers,
        include_dirs: &dirs,
        defines: &[],
        cpp_standard: "c++17",
        from_dir: &std::env::temp_dir(),
        instantiate: &[],
    };
    let h = match import(&req) {
        Ok(h) => h,
        Err(ImportError::Unavailable(e)) => {
            eprintln!("skipping: libclang is not available: {e}");
            return;
        }
        Err(e) => panic!("the two headers did not import: {e:?}"),
    };
    for name in ["Pair", "first", "second"] {
        assert!(h.decl(name).is_some(), "`{name}` was not imported: {h:#?}");
    }
    assert!(
        matches!(&h.decl("ORIGIN").expect("the second header's macro").kind,
            DeclKind::Const { value: ConstValue::Typed(CType::Named(n)), .. } if n == "::Pair"),
        "{:?}",
        h.decl("ORIGIN")
    );
    assert_eq!(h.requested, headers);
    assert!(h.paths[0].ends_with("first.h") && h.paths[1].ends_with("second.h"), "{:?}", h.paths);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A loader such as glad names each function through a macro, `#define glClear
/// glad_glClear`, over a global function pointer: a thousand of them in one header, behind
/// macros that do not compile. Every one is imported, however many there are and however
/// many fail before it (spec §15.2).
#[test]
fn every_macro_naming_a_function_pointer_is_imported() {
    let dir = std::env::temp_dir().join("dz_import_loader");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut text = String::from("#pragma once\n");
    for i in 0..40 {
        text.push_str(&format!("#define BROKEN_{i} no_such_name_{i}\n"));
    }
    for i in 0..1000 {
        text.push_str(&format!(
            "typedef int (*PFNPROC{i})(int);\nextern \"C\" PFNPROC{i} loader_f{i};\n#define f{i} loader_f{i}\n"
        ));
    }
    std::fs::write(dir.join("loader.h"), text).unwrap();
    let dirs = [dir.clone()];
    let req = Request {
        headers: &["loader.h".to_owned()],
        include_dirs: &dirs,
        defines: &[],
        cpp_standard: "c++17",
        from_dir: &std::env::temp_dir(),
        instantiate: &[],
    };
    let h = match import(&req) {
        Ok(h) => h,
        Err(ImportError::Unavailable(e)) => {
            eprintln!("skipping: libclang is not available: {e}");
            return;
        }
        Err(e) => panic!("loader.h did not import: {e:?}"),
    };
    let f = CType::Fn { params: vec![I32], ret: Box::new(I32) };
    for name in ["f0", "f511", "f512", "f999"] {
        match &h.decl(name).unwrap_or_else(|| panic!("`{name}` was not imported")).kind {
            DeclKind::Const { value: ConstValue::Typed(t), from_macro: true } => {
                assert_eq!(*t, f, "{name}")
            }
            other => panic!("{name}: {other:?}"),
        }
    }
    assert!(h.skipped.iter().any(|s| s.name == "BROKEN_39"), "{:?}", h.skipped);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_missing_header_is_reported_with_clang_s_words() {
    let req = Request {
        headers: &["no_such_header.h".to_owned()],
        include_dirs: &[],
        defines: &[],
        cpp_standard: "c++17",
        from_dir: &std::env::temp_dir(),
        instantiate: &[],
    };
    match import(&req) {
        Err(ImportError::Failed(msgs)) => {
            assert!(msgs.iter().any(|m| m.contains("no_such_header.h")), "{msgs:?}")
        }
        Err(ImportError::Unavailable(_)) => {}
        Ok(h) => panic!("imported a header that does not exist: {h:?}"),
    }
}

// ---------------------------------------------------------------------------
// C++ (spec §15.2, §15.4) — `tests/run/cppinterop`'s `shapes.hpp`
// ---------------------------------------------------------------------------

fn shapes() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/run/cppinterop/vendor/shapes/include")
}

fn import_shapes(instantiate: &[(String, String)]) -> Option<std::sync::Arc<dz_cppimport::Header>> {
    let dirs = [shapes()];
    let req = Request {
        headers: &["shapes.hpp".to_owned()],
        include_dirs: &dirs,
        defines: &[],
        cpp_standard: "c++17",
        from_dir: &std::env::temp_dir(),
        instantiate,
    };
    match import(&req) {
        Ok(h) => Some(h),
        Err(ImportError::Unavailable(e)) => {
            eprintln!("skipping: libclang is not available: {e}");
            None
        }
        Err(e) => panic!("shapes.hpp did not import: {e:?}"),
    }
}

#[test]
fn a_namespace_is_a_scope_its_declarations_are_reached_through() {
    // spec §15.2 — `ns::A::B::thing`, and the C++ spelling kept beside the Dazzle name.
    let Some(h) = import_shapes(&[]) else { return };
    let d = |n: &str| h.decl(n).unwrap_or_else(|| panic!("`{n}` was not imported: {h:#?}"));

    assert_eq!(d("Point").scope, vec!["shapes".to_owned()]);
    assert_eq!(d("Point").cpp, "::shapes::Point");
    // A nested namespace adds a segment, on both sides.
    assert_eq!(d("manhattan").scope, vec!["shapes".to_owned(), "geom".to_owned()]);
    assert_eq!(d("manhattan").cpp, "::shapes::geom::manhattan");
    // A type named in a signature carries the C++ path, so two namespaces may each declare
    // a `Point` without either being mistaken for the other.
    match &d("area_of").kind {
        DeclKind::Fn { params, .. } => {
            assert_eq!(params[0].ty, CType::Named("::shapes::Kind".into()))
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(&d("Kind").kind, DeclKind::Enum { scoped: true, .. }));
}

#[test]
fn a_cpp_reference_is_a_dazzle_reference() {
    // spec §15.2 — `const T&` is `&T` and `T&` is `&mut T`. A C++ API takes one almost
    // everywhere a C API would take a pointer, so without this most headers import nothing.
    let Some(h) = import_shapes(&[]) else { return };
    let point = CType::Named("::shapes::Point".into());
    match &h.decl("length_sq").expect("length_sq was not imported").kind {
        DeclKind::Fn { params, .. } => assert_eq!(
            params[0].ty,
            CType::Ref { mutable: false, referent: Box::new(point.clone()) }
        ),
        other => panic!("{other:?}"),
    }
    match &h.decl("translate").expect("translate was not imported").kind {
        DeclKind::Fn { params, .. } => {
            assert_eq!(params[0].ty, CType::Ref { mutable: true, referent: Box::new(point) })
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_class_imports_its_public_member_functions_and_constructor() {
    // spec §15.2 — a member function is a method, `const` decides the receiver, and a
    // second method of the same name is an overload resolved at the call site (§15.3).
    let Some(h) = import_shapes(&[]) else { return };
    let DeclKind::Struct { methods, fields, move_only, .. } = &h.decl("Counter").unwrap().kind
    else {
        panic!("Counter did not import as a struct");
    };
    assert!(!move_only, "Counter has no destructor of its own");
    // `total` is private: it stays in the type as padding, but is not Dazzle's to name.
    assert!(fields.is_empty(), "{fields:?}");

    let ctors: Vec<&dz_cppimport::Method> = methods.iter().filter(|m| m.is_ctor).collect();
    assert_eq!(ctors.len(), 1);
    assert_eq!(ctors[0].params[0].ty, I32);

    let by_name = |n: &str| methods.iter().filter(|m| m.name == n).collect::<Vec<_>>();
    // `int value() const` reads its receiver.
    let value = by_name("value");
    assert_eq!(value.len(), 1);
    assert_eq!(value[0].receiver, Some(dz_cppimport::Receiver::Ref));
    assert_eq!(value[0].ret, I32);
    // `void add(int)` and `void add(Point)` are one overload set.
    let add = by_name("add");
    assert_eq!(add.len(), 2, "{add:?}");
    assert!(add.iter().all(|m| m.receiver == Some(dz_cppimport::Receiver::MutRef)));
    assert_eq!(add[0].params[0].ty, I32);
    assert_eq!(add[1].params[0].ty, CType::Named("::shapes::Point".into()));
}

#[test]
fn a_class_with_a_destructor_and_no_copy_constructor_is_move_only() {
    // spec §15.2 — C++ runs the destructor itself, so Dazzle emits no `drop`; copying it
    // would destroy the same resource twice, so the type is move-only.
    let Some(h) = import_shapes(&[]) else { return };
    let DeclKind::Struct { move_only, methods, .. } = &h.decl("Buffer").unwrap().kind else {
        panic!("Buffer did not import as a struct");
    };
    assert!(move_only);
    // The deleted copy constructor is C++'s own business and is not a method to call.
    assert_eq!(methods.iter().filter(|m| m.is_ctor).count(), 1);
    assert!(methods
        .iter()
        .any(|m| m.name == "fill" && m.receiver == Some(dz_cppimport::Receiver::MutRef)));
}

#[test]
fn a_template_is_imported_only_where_an_instantiation_is_named() {
    // spec §15.4 — templates are not mapped onto Dazzle generics: `[cpp.instantiate]` names
    // the instantiations wanted, and each is an ordinary concrete type.
    let Some(bare) = import_shapes(&[]) else { return };
    assert!(bare.decl("Pair").is_none(), "a template imported without being asked for");
    assert!(bare.decl("IntPair").is_none());

    let wanted = [("shapes::Pair<int>".to_owned(), "IntPair".to_owned())];
    let Some(h) = import_shapes(&wanted) else { return };
    let d = h.decl("IntPair").unwrap_or_else(|| panic!("IntPair was not imported: {h:#?}"));
    // The Dazzle name is the one that was asked for; the C++ spelling is the instantiation.
    assert_eq!(d.cpp, "::shapes::Pair<int>");
    let DeclKind::Struct { methods, .. } = &d.kind else { panic!("{:?}", d.kind) };
    // The template's parameters are bound to this instantiation's arguments, so `T` is
    // `int` in every member it has.
    let ctor = methods.iter().find(|m| m.is_ctor).expect("no constructor");
    assert_eq!(ctor.params.len(), 2);
    assert!(ctor.params.iter().all(|p| p.ty == I32), "{ctor:?}");
    let sum = methods.iter().find(|m| m.name == "sum").expect("no `sum`");
    assert_eq!(sum.ret, I32);
}
