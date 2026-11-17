---
id: T-0165
title: "words: scope regions make it 5% slower with g++, where clang++ is 30% faster"
milestone: M15
status: done
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

**Measured again 2026-11-17, and closed: not the regions' cost.** At `a2a3b20`,
`tools/bench_regions.py`, eleven alternating runs of each build, twice:

| `words` | heap | regions |
|---|---:|---:|
| clang++ | 137 ms | 134 ms |
| g++ | 138 ms | 142–143 ms |

The clang++ heap build was the outlier, and is gone (251 → 137 ms). On g++ regions are
still about 3% slower, and the same sources show it is not where the memory goes:

- **Regions run fewer instructions** on both compilers (callgrind, 300,000 words): clang++
  311.1 M → 306.4 M, g++ 325.7 M → 323.5 M. Page faults and system time are the same, and
  cachegrind's simulated cache misses and branch mispredictions are within 0.3%.
- **The time is in the counting loop** (the program timed by phase, fifteen runs each): g++
  73.4 ms on the heap, 75.6 in regions; clang++ 66.0 and 62.6, faster in regions.
- **It stays with every allocation on the heap.** The regions build with the map, its keys,
  the `word` buffer and the text each put back on the heap, by editing the generated C++
  before g++ compiled it, and with `word` and `text` as ordinary C++ locals rather than
  storage C++ never destroys: 75–78 ms, never the heap build's 73. Nor is it `generate`'s
  inlining, which regions change: forced either way, neither build moves. Nor loop or
  function alignment (`-falign-loops=32`, `=64`, `-falign-functions=64`): the gap stays.
- **The code the loop runs is the same C++.** `std`'s and the program's `HashMap` and
  `String` methods differ between the builds only in constructors, which take the
  placement; `find`, `get_mut_str`, `push` and `place` are the same text.
- **Without LTO it is gone**: g++ 149.5 ms on the heap, 150.1 in regions (`-fno-lto`), the
  counting loop 79.6 and 78.8.

So the 3% is g++'s whole-program optimisation laying out or inlining `main` differently
when `main` holds a region, with no instruction more to run. Without hardware counters
(`perf_event_paranoid` is 3 on this machine) what it does differently cannot be seen; with
them, `perf stat` on the two g++ builds would say whether it is the front end or
something else. Not worth more while regions run less code and are level without LTO.
