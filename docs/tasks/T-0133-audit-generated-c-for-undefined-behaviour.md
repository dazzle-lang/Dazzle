---
id: T-0133
title: An audit of the generated C++ for undefined behaviour
milestone: none
status: done
kind: feature
area: codegen
---

Part of [D-0094](../decisions/D-0094-safe-code-has-no-undefined-behaviour.md)'s promise;
the audit itself needs no decision, though a finding may.

**Where the code is**

| Piece | Where |
|---|---|
| The audit: each operation, what C++ leaves undefined, what rules it out | "What the generated C++ relies on" in `docs/testing.md` |
| The flags (`-fwrapv`, `-fno-finite-loops`, `-fno-strict-aliasing`) | `compile` in `crates/dz/src/build.rs`; spec §16.5 |
| Defined arithmetic | `num::div`, `rem`, `shl`, `shr`, `float_to_int` in `crates/dz_cpp/src/runtime.rs`; `defined_op` in `crates/dz_cpp/src/lib.rs` |
| Trivially copyable, checked against C++ | `assert_trivially_copyable` and `union_decl` in `crates/dz_cpp/src/lib.rs` |
| The shim's C library calls | `panic_len`, `file_read`, `text_parse_f64`, `process_env`, `write_stream` in `crates/dz_cpp/src/runtime.rs` |

**Done** (2026-11-04). Every operation the emitter and the shim produce was listed against what
C++ leaves undefined for it; the table is in `docs/testing.md`, and spec §16.5 points to it.
Most were already ruled out (T-0128, T-0129, T-0132). What the audit found:

- **A loop with no side effects was deleted.** `while x < 10 {}` with `x` never changing
  returned past the loop at `-O2` on both clang++ and g++, since C++ lets an optimiser assume
  such a loop ends. Fixed with `-fno-finite-loops`; §1.1 now says a loop runs as written.
- **Strict aliasing.** `std`'s trusted bodies read memory as another type (the memory
  tracker scans a value's bytes as `usize` words, and `TieGuard` reads a `T*` field as a
  `const void*`). Fixed for them and for every `unsafe` block with `-fno-strict-aliasing`;
  §10.6 says a reinterpreted pointer reads the bytes as the type it names. Neither flag moved
  the whole-program benchmarks beyond their noise.
- **A union Dazzle calls trivially copyable that C++ did not.** One carrying a struct with a
  constructor had hand-written copy and move members, so `List`'s `memmove` of it was outside
  C++'s object model. It now uses C++'s own, and the emitter asserts
  `__is_trivially_copyable` for every type Dazzle calls trivially copyable, so the two
  cannot drift apart unseen. The whole run corpus passes the assertion.
- **The shim handed C a null pointer with a count of 0** (an empty `String`'s text) in
  `panic_len`, `file_read`, `text_parse_f64`, `process_env` and `write_stream`. Each checks
  the count now.
- **A static's field default can run code before `main`** that reads a static C++ has not
  built: [T-0166](T-0166-static-field-defaults-run-code-before.md), closed the same day by
  building such statics from `main` in a fixed order (r116). On the way: [T-0167](T-0167-field-default-that-calls-free-function.md), a field
  default calling a free function does not compile.
- §10.6 also says what narrowing a signed integer and `f64 as f32` do, which C++17 leaves to
  the compiler and every supported one defines.

Checked and found sound: the suspects below about `switch` (the end of a function is a
panic, and a `switch` used as a value starts from a default), C enums (the C boundary, and
T-0048), and `offsetof` (supported by both compilers on what Dazzle emits).

**What it is.** Every construct `dz_cpp` emits lands on C++, which leaves many inputs
undefined. The review behind D-0094 found two such places by chance
([T-0128](T-0128-or-unreachable-undefined-behaviour-in-release.md),
[T-0129](T-0129-division-by-zero-min-1-oversized.md)). An audit lists every C++ operation
the emitter (`dz_cpp/src/lib.rs`) and the runtime shim (`dz_cpp/src/runtime.rs`) produce,
the inputs C++ leaves undefined, and how Dazzle rules each one out: a check, a proof, or
`unsafe`.

**Suspects to start with.**

- **`memcpy` or `memmove` with a null pointer and a size of 0.** This is undefined in C and
  C++, and an empty `List` has a null array. Look at `List`'s shifts and at the
  allocators' `realloc`.
- **Strict aliasing.** A pointer cast in `unsafe` can read one type as another.
  `-fno-strict-aliasing` costs little and removes the whole class; the Linux kernel builds
  with it.
- **Flowing off the end of a function that returns a value.** An exhaustive `switch`
  counts as returning (§6.4). Check what is emitted after it, for a union whose tag C or
  `unsafe` code set to a value no variant has.
- **Loops without side effects.** C++ lets the optimiser assume such a loop ends. clang 21
  kept `while true {}`; `while x < 10 {}` with `x` unchanged may not survive.
- **C enums.** An imported enum may hold any value
  ([T-0048](T-0048-imported-c-or-c-enum-variants.md)).
- **Object lifetime.** `memcpy` of a type that is not trivially copyable (std checks
  `is_trivially_copyable`), and reading a C++ union's inactive member.
- **Signed narrowing** (`i32 as i8`). It is implementation-defined in C++17 and defined
  since C++20, so it is fine on every supported compiler; §10.6 should say so.
- **`offsetof` on a type that is not standard-layout** (reflection, §13). C++17 only
  conditionally supports it.

**Why it matters.** Dazzle's promise is only as good as its weakest emitted operation, and
C++ undefined behaviour does not stay local: the optimiser reasons backwards from it, as
T-0128's loop shows.

**What it would take.** A table in this file or in `docs/testing.md`: each operation, its
undefined inputs, and the guard. Each finding becomes a guard in codegen, a rule in the
checker, or a line in the spec saying it is `unsafe`.
