---
id: T-0164
title: "Peak memory over C++ in astar and words: HashMap's open-addressing layout, and short Strings"
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
| `HashMap`'s slots (a 32-bit hash, a key, a value, in three arrays) and its growth past three quarters | `grow_to`, `key_hash` in `dz_project/src/std/collections/hash_map.dz` |
| Peak memory on the heap against regions | `tools/bench_regions.py` |
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

**Measured** (2026-11-04). The guess above was wrong: regions are not the cause.
`tools/bench_regions.py` gives `astar` the same peak on the heap as in regions (19.3 and
19.4 MiB), and `words` a higher one on the heap (44 against 41). `astar` instrumented: its
largest search holds 110,069 entries in each map, and both maps grow to 262,144 slots, 42%
full. While `came_from` grows, its old arrays are alive beside the new ones. With 20- and
24-byte slots that is about 14.6 MB of the peak; the runtime's baseline is 3.5 MiB.

Done: a slot keeps a 32-bit hash, from 64, and `astar` peaks at 17.4 MiB, `words` at 40.

Left, each a library design question:
- **`astar`: the layout.** A map that fills further before growing needs more than linear
  probing to stay fast there: a control byte per slot probed in groups, as SwissTable does,
  or Robin Hood ordering. The old arrays alive during growth are the cost of open
  addressing, against `std::unordered_map`'s node per entry.
- **`words`: each short word's own allocation** was 2 MiB of it: with
  [T-0156](T-0156-string-always-allocates-short-text-kept.md) done, `words` peaks at 37.9
  MiB against 32.3. What is left is the map's layout, as for `astar`.
