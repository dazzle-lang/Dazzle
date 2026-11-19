---
id: T-0256
title: MSVC in CI, warning-clean at /W4
milestone: M21
status: done
kind: feature
area: tooling
where: `.github/workflows/ci.yml` (the `msvc` row of the test matrix, `DZ_CXX: cl` and `DZ_CXXFLAGS: /WX`; the `Test (msvc)` step); `crates/dz/src/toolchain.rs` (`generated_code`: `/W4` and the warnings turned off, each with its reason); `crates/dz_cpp/src/runtime.rs` (`if constexpr` in `num::`); `crates/dz_cpp/src/lib.rs` (`expr`'s `Unary` arm: the smallest `i32` and `i64` literals); `crates/dz/tests/run.rs` (`every_example_builds` includes Box2D through its upstream CMake project)
---

M21's criterion. Invariant 7 says generated C++ compiles warning-clean on more than one
compiler; once `cl.exe` is supported it is one of them, and only CI keeps it so.

## Windows CI coverage (2026-11-19)

**The CI row.** `msvc` on `windows-latest`: `DZ_CXX: cl`, and `DZ_CXXFLAGS: /WX`, which makes
every warning `cl.exe` gives at `/W4` an error in every program the suite builds, at the
optimisation level it is built with. It runs the `run`, `matrix` and `incremental` integration tests, plus
`dz test` and `dz bench`. The steps that were for the
`windows` row by name (libclang, Ninja) are now for any Windows runner. The row has not run
yet: it lands with this change, and its first run is the check.

**Fixed where the generated code was at fault.**

- C4127, a constant condition: the runtime's arithmetic asked `if (is_signed<T>())` and
  `if (sizeof(T) < 8)`; they are `if constexpr` now, with `else` branches.
- C4146, unary minus on an unsigned type: the literal `-2147483648`. C++ has no literal for
  `i32`'s or `i64`'s smallest value (`2147483648` is read as a wider or unsigned type before
  it is negated), so it is written `(-2147483647 - 1)`.
- D9024, an object `cl.exe` does not recognise: objects are named `.obj` under it.

**Turned off, each with its reason** in `toolchain::generated_code`:

- C4702, unreachable code. At `/O2` it is code the optimiser found dead after inlining, a
  bounds check's panic known to fail in one caller, reported against the inlined
  function's lines; at `/Od` it is the trap `dz` writes after a body whose every path
  returns where C++ cannot always see it. Neither is a mistake. (Removing the emitter's dead
  code after a folded `if T::is_relocatable { …; return }` was tried and dropped: it did not
  touch the `/O2` cases.)
- C4723, potential divide by 0: only ever a float, which IEEE 754 defines; an integer
  division with a divisor that can be zero goes through `num::div`, which panics.
- C4458 and C4459, a local shadows a field or global: a local may be named like a constant, static or
  function of its module, and every use of the item is written qualified (T-0121).

**Measured** with MSVC 14.50, every `tests/run` program built by `dz build --cxx cl` with
`/WX`, debug and release: all warning-clean, the CMake one aside (no Ninja on that machine).
Before, with MSVC 14.38 at `/O2` over all files: C4702 3,469, C4127 1,146, C4723 4, C4459 2,
C4146 1.

The first hosted CI run is pending. Box2D uses its upstream CMake settings for the AVX2
solver and is included in the example suite.

Imported headers: libclang already parses with the MSVC target on Windows (T-0020), so
`import cpp` reads the same headers `cl.exe` compiles against; check that nothing in the
import cache assumes the GCC family.
