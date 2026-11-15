---
id: T-0233
title: main's outermost region holds memory the heap would have reused
milestone: M15
status: open
kind: design
area: placement
where: `crates/dz_flow/src/placement.rs`: the proposer that puts a value in its block's region; `crates/dz_project/src/std/memory/scope_arena.dz`: `ScopeArena`, `SCOPE_CHUNK`, `grow`; `tests/bench_programs/json/`
---

**A design question for the user**, measured 2026-11-14 under T-0225. With regions on,
`json` peaks at 195.6 MiB; with every value on the heap (`DZ_REGIONS=0`) at 185.6, against
its C++ half's 180. Sampling resident memory every half millisecond, the gap opens while the
tree is turned into records and stays to the end, yet the live heap `massif` counts there is
no larger with regions (171 MiB against 178, at slightly different moments). The evidence
points to reuse: in the heap build, each record's small tag list is allocated into a hole
that freeing the parse's temporaries left in `malloc`'s heap; in the region build it goes
into a fresh region chunk, `main`'s, which touches new pages while those holes sit unused.

**What was ruled out.** Outgrown small buffers left in the region: a region that reused its
freed small blocks by size class changed nothing on any benchmark and was not kept. Chunk
capacity alone: an untouched part of a chunk is not resident.

**The question.** A region pays when it is released before the program ends, or when it
saves the drops of what it holds. `main`'s outermost region is released only at exit, so
what it holds gains no early release and loses the heap's reuse of freed memory. Options to
weigh, consistency first (regions are architecture, not only speed):

- **Keep it**: every value has a known home, and a skipped drop at exit is still saved work.
- **Place `main`'s outermost block on the heap**, or any region that lives as long as the
  program: the program's memory as C++'s would be, at the cost of a home that is "the heap"
  by rule rather than by proof.
- **Feed a region's first chunk from memory the heap has freed**, or give it smaller chunks
  to start, so it touches fewer new pages.

Measure each on `json` (memory and time, both compilers) and on every benchmark.

**2026-11-15, time as well as memory (T-0234).** Timed by phase with `DZ_REGIONS=0`
against the default, both on the hand-patched build T-0234 ends its table with: parsing 95
ms against 101 on clang++ and 94 against 98 on g++, converting 30 against 28 and 31 against
29, the whole run 4–5 ms faster on both and the peak 187 MiB against 196. One thing regions
do not explain: the convert phase takes about 3,800 page faults against the C++ half's
2,000, and 2,250 when the records' tags lists are not built, but as many with regions off
(3,950), so the tags lists' fresh pages are not the region's doing.

## Where the code is

| What | Where |
|---|---|
| Which block a value's region is | `crates/dz_flow/src/placement.rs`, the proposer |
| The region arena, its chunks | `crates/dz_project/src/std/memory/scope_arena.dz`, `scope_pool.dz` |
| The measurements | T-0225's progress notes |
