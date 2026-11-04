---
id: T-0164
title: "Peak memory 30-50% over C++ in astar and words: a region keeps every small block freed into it"
milestone: M15
status: open
kind: feature
area: ownership
---

Found while working on [T-0157](T-0157-astar-each-search-hash-maps-regrow.md), 2026-11-04,
and not yet measured: the explanation below is a likely cause, to be confirmed first.

**Where the code is**

| Piece | Where |
|---|---|
| A region's `free`: a large block goes back, a small one stays in its chunk | `free` and `free_large` in `dz_project/src/std/memory/scope_arena.dz` |
| `HashMap`'s growth, which frees the old arrays | `grow_to` in `dz_project/src/std/collections/hash_map.dz` |
| The numbers | `tests/bench_programs/RESULTS.md` |

**What it is.** `astar` peaks at 19.4 MiB against the C++ half's 12.7, and `words` at 41
against 32 (`expr` is under C++). A region frees nothing small until it ends: a
`HashMap` or `List` that grows frees its old arrays into the region, where they stay. A
search in `astar` grows two maps about eleven times, so each search's region holds every
size each map has been.

**What it would take.** Measure first: peak memory with `DZ_REGIONS=0` against the default
build (`tools/bench_regions.py` reports both), and where the bytes are. If it is the
grown-out arrays, either a region reuses a freed small block of the same size class, or a
container that grows puts its arrays on the heap once they pass a size. Compiler and
library work (`docs/log/`).
