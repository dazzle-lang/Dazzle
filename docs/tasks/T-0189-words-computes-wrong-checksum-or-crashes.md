---
id: T-0189
title: words computes the wrong checksum, or crashes with regions off, under g++ LTO with a larger inline budget
milestone: M15
status: open
kind: bug
area: codegen
where: unknown yet: the generated C++ for `tests/bench_programs/words`, the runtime (`crates/dz_cpp/src/runtime.rs`), or g++
---

**What is wrong.** Found on 2026-11-09 while measuring T-0185. `tests/bench_programs/words`,
built `--release` with g++ 14.2 and `DZ_CXXFLAGS="--param max-inline-insns-auto=70"` (which
lets g++ inline more), prints checksum `12921413855635122013` where the C++ half and every
other build print `18012059585114795422`. With `DZ_REGIONS=0` (every value on the heap) it
crashes with a segmentation fault, even on the small input. The code generated before
T-0185 does the same, so T-0185 did not cause it.

| Build of `words`, g++ 14.2 | Result |
|---|---|
| `--release` | right |
| `--release`, the larger budget | wrong checksum (full input; the small one is right) |
| the same, `DZ_REGIONS=0` | segmentation fault |
| the larger budget without LTO (`-fno-lto`) | right |
| `-O3` | right |
| the larger budget with `-fsanitize=address,undefined` | right, nothing reported |
| clang++ 19, `-mllvm -inline-threshold=2000` | right |

**Why it matters.** Either the generated C++ or the runtime has undefined behaviour that a
little more inlining exposes, or g++'s LTO has a bug. The first would mean any program can
go wrong when a compiler's heuristics change, which invariant 7 forbids. Neither can be left
unexplained.

**What it would take.** Build the `DZ_REGIONS=0` crash with `-g` added and find where it
dies; then reduce: drop LTO partitioning (`-flto-partition=one`), bisect which functions
must be inlined for it to happen (`__attribute__((noinline))` on suspects), and read the
code there for an object used after its lifetime ends, an uninitialised read, or a
reference into storage that moved. If it reduces to a program with no UB, report it to GCC.

