---
id: T-0157
title: "astar: each search's hash maps regrow about eleven times, and its Hash and Ord calls are not inlined"
milestone: M15
status: open
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
