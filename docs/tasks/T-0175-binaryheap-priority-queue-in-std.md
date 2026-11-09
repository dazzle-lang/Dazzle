---
id: T-0175
title: BinaryHeap: a priority queue in std
milestone: M12
status: open
kind: feature
area: stdlib
---

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
