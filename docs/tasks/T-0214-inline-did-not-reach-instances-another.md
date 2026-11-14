---
id: T-0214
title: #[inline] did not reach instances of another module's generics, so a loop over a List ran 8x slower than over a slice
milestone: M15
status: done
kind: bug
area: codegen
where: `crates/dz_cpp/src/lib.rs`: `inline_fns`, `Emitter::is_inline`, `only_stores_params`; `std/collections/list.dz`, `slice_iter.dz`, `slice_iter_mut.dz`
---

**Done** (2026-11-14). Found while asking whether bounds checks cost in loops over the
benchmarks (they do not: see below).

**Optimised builds only, since the same day.** In every module's header, those bodies made
a debug build of the 100-module scale project from clean 8.1 s to 12.0 s, every module
parsing its dependencies' `List` accessors again, for nothing at `-O0`. A debug build now
defines them once, out of line (`dz_cpp::Options::inline_instances`, `dz::build::for_build`;
`docs/log/tooling.md`).

| Piece | Where |
|---|---|
| The `#[inline]` set, now every module's, built once in `emit_selected` | `inline_fns` in `crates/dz_cpp/src/lib.rs` |
| Which bodies go in the header | `Emitter::is_inline`, `only_stores_params` |
| The accessors marked | `List::len`, `is_empty`, `as_slice`, `as_mut_slice`, `index`, `index_mut`, `iter`, `iter_mut`; `SliceIter` and `SliceIterMut`'s `valid`, `get`, `advance` |
| Test | `another_modules_inline_reaches_its_instances` in `crates/dz/tests/run.rs` |

**What was wrong.** The emitter collected the functions written `#[inline]` from the files of
the module it was emitting, and nothing else. An instance of another module's generic is
emitted by the module that needs it (T-0185), so `std`'s `#[inline]` never reached a
`List<i32>` a program instantiates: its bodies went to the program's `.inst.cpp`, apart from
the loops calling them. Link-time optimisation inlines them at link, after clang++'s
per-file pipeline has shaped the loop, and the loop was not vectorized. `List`'s accessors
were not written `#[inline]` either, so it would not have helped yet.

The second half was `SliceIter`'s constructor, which every `for &x in list` calls. A
constructor cannot be written `#[inline]` (spec §12.8: a function or a method), and
compiled out of line it took the iterator's address out of the loop's
sight: clang++ then stored the iterator's index to memory on every pass. A constructor that
does nothing but store its parameters into fields now goes in the header, as C++ would
write it inside the class. That is where a body is compiled, which changes nothing a
program does, so it is not a language change.

**Measured** on 100 000 `i32`s, `dz build --release`, clang++ 19, µs per pass:

| Loop | before | after |
|---|---:|---:|
| `for i in 0..xs.len() { t += xs[i] }` on a `&List` | 22.7 | 3.2 |
| `for &x in xs` on a `&List` | 20.3 | 2.6 |
| the same over a slice, bounds-checked | 2.6 | 2.6 |
| the same over a slice, `unsafe` and unchecked | 2.6 | 2.6 |

A bounds check in a loop over `0..xs.len` costs nothing, as §5.3 says: the optimiser removes
it once it can see the length. What cost was not seeing the body. g++ 14 vectorizes none of
these loops at `-O2` and was never slower for a `List` than for a slice; one of its copies
measured twice as slow, which was the loop straddling a 64-byte boundary in the harness, the
instructions being the same.

The whole-program benchmarks do not move (`astar`, `expr`, `words` within noise on both
compilers, build times the same): none of them walks a `List` in order in its hot path.

**Left, and done by [T-0215](T-0215-loop-writing-through-mut-list-reloads.md).** Writing
through a `&mut List` in a loop, `dst[i] = src[i] * 3`, was still 5× the same loop over
slices on clang++, even with every body visible: after each store C++ could not prove the
list's length and data pointer unchanged. T-0215 indexes a slice taken before the loop
wherever the tie analysis proves nothing in it kills the list.
