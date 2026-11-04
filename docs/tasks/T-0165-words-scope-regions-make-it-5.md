---
id: T-0165
title: "words: scope regions make it 5% slower with g++, where clang++ is 30% faster"
milestone: M15
status: open
kind: feature
area: ownership
---

Found 2026-11-04, measuring what placement is worth once both halves of the whole-program
benchmarks ran the same algorithms. `tools/bench_regions.py`, nine alternating runs of
each build, the same source with `DZ_REGIONS=0` (every value on the heap) and `1` (the
default):

| `words` | heap | regions |
|---|---:|---:|
| clang++ | 251 ms | 176 ms |
| g++ | 200 ms | 210 ms |

`astar` is the same either way, and `expr` 3–7% faster in regions on both compilers.
`words` is the one where regions lose, and only on g++; and its heap build on clang++ is
the slowest of the four, a quarter slower than g++'s.

**Where the code is**

| Piece | Where |
|---|---|
| The program | `tests/bench_programs/words/src/main.dz` |
| The measurement | `tools/bench_regions.py` (`--cxx clang++,g++ --no-loop words`) |
| The region arena and its allocation | `std/memory/scope_arena.dz`, `scope_pool.dz` |
| Where codegen calls it | `crates/dz_cpp/src/lib.rs`, placement (`#placement`, region frames) |

**What it would take.** Measure first: `perf` both builds on both compilers, and compare
where the time goes: the map's growth, each key's `String` copy, the sort. The clang++
heap build may be the outlier rather than the g++ region build. Compiler and library work,
no language change.
