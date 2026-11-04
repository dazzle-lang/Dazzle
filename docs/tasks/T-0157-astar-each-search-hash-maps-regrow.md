---
id: T-0157
title: "astar: each search's hash maps regrow about eleven times, and its Hash and Ord calls are not inlined"
milestone: M15
status: done
kind: feature
area: codegen
---

A `gprof` profile of `tests/bench_programs/astar` (2026-11-03, regions on, after T-0153
and T-0155) puts most of the time in the search itself, and then:

- `HashMap::grow_to`: 2,225 calls over 200 searches, about eleven per search, 9% of the
  time. Each search's maps start empty and double their way up, rehashing every key each
  time.
- `Hash for Cell` (11 million calls, 15%) and `Ord for Node` (50 million calls, 7%) are
  called, not inlined, although they and the `HashMap` and `MinHeap` instances that call
  them are in the same translation unit. That profile was built without link-time
  optimisation (T-0154), which may already inline them in a real release build: check the
  release binary first.

`astar` is 1.12-1.19 times C++ (2026-11-03).

**Where the code is**

| Piece | Where |
|---|---|
| The program | `tests/bench_programs/astar/src/main.dz` |
| `HashMap`'s growth (`grow_to`) and its starting capacity | `dz_project/src/std/collections/hash_map.dz` |
| Generic instances, emitted inline in the header; impl functions in the module's `.cpp` | `instance_definitions`, `impl_definitions` in `dz_cpp/src/lib.rs` |
| The panics moved out of `List`'s `index`, `index_mut` and `swap` | `list_index_out_of_bounds`, `list_swap_out_of_bounds` in `dz_project/src/std/collections/list.dz` |

**Done** (2026-11-04). The release build, with link-time optimisation, already inlines
`Hash for Cell` and `Ord for Node`. What it did not inline was `List::index_mut` (100 million
calls, 15% of the time) and `List::swap` (32 million, 5%), in the heap's sift loops: each
formatted its out-of-bounds panic inline, which made a one-compare accessor too large to
inline. The panics are now two small functions of their own, and `index`, `index_mut` and
`swap` are inlined. `astar` went from 1.07–1.19× its C++ half to 0.84× on clang++ and 0.95×
on g++; `words`, which indexes lists too, from 1.15–1.27× to 1.11× and 1.07×.

The regrowth was left as it is. `HashMap` doubles from eight slots at three-quarters full,
as the C++ half's `unordered_map` regrows too, and after the inlining `grow_to` is about 2%
of the time. What it may cost instead is memory: a region keeps every grown-out array
until it ends, which is likely why `astar` peaks 50% over C++. That is
[T-0164](T-0164-peak-memory-30-50-over-c.md).
