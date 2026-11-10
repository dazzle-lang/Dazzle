---
id: T-0189
title: words computes the wrong checksum, or crashes with regions off, under g++ LTO with a larger inline budget
milestone: M15
status: done
kind: bug
area: codegen
where: `destroy` and `move_out` in `crates/dz_cpp/src/runtime.rs`; `set_len` and `make_room` in `std/text/string.dz`
---

**Done** (2026-11-09). Undefined behaviour in the runtime, not a g++ bug. `String` grows by
storing its new capacity into `m_store[1]` (`make_room`), and every edit then ends with
`destroy(m_store[1])` (`set_len`): a no-op on a word, there as a signal to the ties check
(spec §6.7). The runtime's `destroy` was `x.~T()`, which on a `size_t` is a pseudo-destructor:
it ends the object's lifetime. g++ treats a store followed by the end of the object's
lifetime as dead (its lifetime DSE) and deletes it. Once it inlined `make_room` and `set_len`
into `push_str`, the capacity was never stored, and the next append wrote past the end of the
text. A watchpoint on the corrupted heap bytes found the write in `set_len`.

`destroy` now runs a destructor only for a class or a union (`__is_class`, `__is_union`,
which GCC, Clang and MSVC all have, being what `<type_traits>` is built on), and `move_out`
goes through it. Nothing changes for a type with a destructor; a number, a pointer or an enum
has none to run. With the larger inline budget, `words` now prints the right checksum, with
regions on and off.

The test, `destroying_a_word_keeps_what_was_stored_in_it` in `crates/dz/tests/run.rs`,
compiles the pattern against the runtime itself with every compiler there is, at -O2: with
the old `destroy`, g++ printed 0 for a word just set to 41, and warned the object was used
uninitialised.

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

