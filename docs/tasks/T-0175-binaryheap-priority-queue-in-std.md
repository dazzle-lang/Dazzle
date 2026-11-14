---
id: T-0175
title: BinaryHeap: a priority queue in std
milestone: M12
status: done
kind: feature
area: stdlib
---

**Done** (2026-11-14, r125): the largest first, with `Reverse<T>` for the smallest, the
user's choice (D-0098). `astar` uses `BinaryHeap<Reverse<Node>>` instead of its own
55-line heap: the same checksum, and as fast with g++ (329 ms to 331 ms, release, median of
nine, alternated); with clang++ 324 ms to 331 ms, 2%, a call to `Node`'s `cmp` through
`Reverse`'s that LTO can inline. Building one from a `List` needed a checker fix: a
constructor parameter `items: List<T>` fitted any argument, so `BinaryHeap<T>(16)` was
ambiguous (§7.2).

## Where the code is

| What | Where |
|---|---|
| the heap | `crates/dz_project/src/std/collections/binary_heap.dz` |
| `Reverse` | `crates/dz_project/src/std/core/reverse.dz` |
| listed in `std` | `crates/dz_project/src/lib.rs`, `STD_SOURCES` |
| choosing a constructor by a generic parameter's shape | `crates/dz_check/src/check.rs`, `pick_by_args` and `shape_fits` |
| tests | `tests/run/binary_heap`; `tests/errors/constructor_generic_parameter_shape.dz` and `_ambiguous.dz` |

Planned for 0.2 by the user on 2026-11-09.

**What it is.** A `BinaryHeap<T: Ord>` in `std::collections`: `push`, `pop`, `peek`, `len`,
`is_empty`, `clear`, and building one from a `List`. Today a program writes its own:

```dz
struct MinHeap<T: Ord> { m_items: List<T> ... }   // tests/bench_programs/astar, 55 lines
let mut open = BinaryHeap<Node>()                  // what it would be
```

**Why it matters.** Path finding (A*, Dijkstra), event queues ordered by time and job
scheduling all need one, and `astar` shows that a user has to write one, ties and all.

**The decision for the user: which element comes out first.** It is a library API, not a
language change.

1. **The largest, as C++'s `priority_queue` and Rust's `BinaryHeap`**, with a
   `Reverse<T>` wrapper whose `Ord` is reversed for the smallest. Familiar to both
   audiences. Recommended.
2. **The smallest**, which is what path finding wants, but surprises anyone coming from
   either language.
3. **Given by a comparison at construction**, as `sort_by` is. Most flexible, and the
   comparison has to be stored.

**What it would take.** `std/collections/binary_heap.dz` (and `Reverse` beside `Ordering`),
listed in `STD_SOURCES` in `crates/dz_project/src/lib.rs`, with ties as `astar`'s heap
declares them (`tie @items = m_items@items`; `push` and `pop` kill it). A
`tests/run/binary_heap` program, and `astar` switched to it, which also checks it is as fast
as the hand-written one.
