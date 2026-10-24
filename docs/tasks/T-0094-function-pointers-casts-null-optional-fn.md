---
id: T-0094
title: Function pointers: casts, null, optional fn, and C's function-pointer typedefs and globals
milestone: M12
status: open
kind: design
area: interop
where: `dz_check/src/check.rs` (`check_cast`, comparisons), `dz_parse` (the `(fn…)?` type), `dz_cppimport/src/clang.rs` (`typedef`, the walk's `scope`)
---

**What it is.** Dazzle has a function-pointer type, `fn(i32, f32) -> bool` (§5.4). A
Dazzle function can be stored in one, called through it, and handed to C as a callback.
Everything else a C programmer does with function pointers is missing. Found writing
`examples/sdl3` on 2026-10-24, each case checked against `dz check`:

| What | Written | Today |
|---|---|---|
| Cast one function type to another, as a loader must: `GetProcAddress` returns `void (*)(void)` and the caller knows the real type | `p as fn(i32, i32) -> i32` | **DZ0511** cannot cast |
| Compare with null: a C callback field or a failed lookup is `NULL` | `if p == null` | **DZ0514** cannot compare `fn() -> void` with `^mut {unknown}` |
| Pass null where C takes an optional callback: raylib's `SetTraceLogCallback(NULL)` | `c::set_cb(null)` | **DZ0501** |
| An optional function, which §20 spells `(fn(i32) -> bool)?` | `let f: (fn(i32, i32) -> i32)? = add` | **DZ0501**: `(fn…)` parses as a one-element **tuple**, so neither storing nor unwrapping works. This is a bug, whatever is decided |
| A function pointer to or from a raw pointer, in `unsafe` | `p as ^void` | **DZ0511** |
| A C function-pointer typedef by name, `PFNGLCLEARPROC`, `GLADloadfunc` | `let f: c::AddProc` | **DZ0301**: not imported, deliberately (`typedef()` imports only struct, enum and union aliases) |
| A header's global variable, glad's `glad_glClear` or a library's `extern int counter` | `c::add_ptr(1, 2)` | **DZ0301**: global variables are not imported at all |

A loader written in Dazzle today has to pun pointers, as `examples/sdl3`'s first `gl.dz` did:

```dz
fn load<F: Copy>(slot: &mut F, name: cstr) {
    let raw = sdl::SDL_GL_GetProcAddress(name)
    let p: ^fn() = &raw
    unsafe {
        if (p as ^usize)^ == 0 { panic!("no {}", name as str) }
        slot = (p as ^F)^                   // what `raw as F` should say
    }
}
```

**Why it matters.** Function pointers are how C libraries are configured and extended:
callbacks with an optional `NULL`, plugin and driver tables, `dlsym` and `GetProcAddress`,
OpenGL and Vulkan loaders, and a game's own tables of state handlers. glad works today only
because each function is also a macro, and macros are imported (fixed on 2026-10-24: a cap of
512 dropped the rest). A Vulkan loader, `dlsym`, or a header whose globals have no macro
in front of them cannot be used from Dazzle without the pun above. The optional-function bug
also makes any optional callback field impossible in Dazzle's own structs.

**Why it is this way.** `fn` types were added for callbacks into C (RGFW, sokol), and nothing
past that was needed until now. `check_cast` allows only numeric and pointer casts.
[T-0015](T-0015-declarations-left-out-import.md) lists function pointers as left out of
imports; parameters and fields of function-pointer type are now imported (spec §15.2 table,
`a_callback_imports_as_a_function_type`), so that row is out of date. Typedefs and globals are
still left out.

**What it would take.** A language change: a decision file and a §21 row, written when this
is implemented. Settled so far, and what is still the user's:

1. **Is `fn` nullable? Settled by the user on 2026-10-24: no.** `fn(i32) -> bool` is never
   null; `(fn(i32) -> bool)?` is the nullable one, as §20 already spells it, and is a plain C
   function pointer with `none` as `NULL`, as `^T?` is for pointers. A value of a
   non-optional type stays always callable, at no run-time cost. Still to settle with it:
   whether an imported callback parameter becomes `(fn…)?` (every C callback may be `NULL`)
   or stays `fn…` with `null` refused, and whether `null` is accepted for `none` there.
2. **Which casts?** Between function types (`p as fn(i32) -> i32`), and between `fn` and
   `^void`/`usize`. Probably all `unsafe`, as the other pointer reinterpretations are.
3. **Typedefs and globals in imports.** Import a function-pointer typedef as an alias of its
   `fn` type (`c::AddProc` is `fn(i32, i32) -> i32`), and a header's global variables as
   module-level statics (`c::counter`, read and written as `static` ones are, §4).
4. Fix the parser so `(fn(i32) -> bool)?` is an optional function, with a test in `typed/`
   and one in `typeerrors/`, whatever 1 decides.

Files: `dz_check/src/check.rs` (`check_cast`, comparison with `null`/`none`), `dz_parse`
(the parenthesised type), `dz_cppimport/src/clang.rs` (`typedef`, a `CXCursor_VarDecl` arm in
the walk), `dz_cpp` (a cast spelled `reinterpret_cast`, as imported constants now are).
