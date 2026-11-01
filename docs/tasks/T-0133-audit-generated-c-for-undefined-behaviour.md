---
id: T-0133
title: An audit of the generated C++ for undefined behaviour
milestone: none
status: open
kind: feature
area: codegen
---

Part of [D-0094](../decisions/D-0094-safe-code-has-no-undefined-behaviour.md)'s promise;
the audit itself needs no decision, though a finding may.

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
