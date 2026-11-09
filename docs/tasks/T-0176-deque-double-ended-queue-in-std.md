---
id: T-0176
title: Deque: a double-ended queue in std
milestone: M12
status: open
kind: feature
area: stdlib
---

Planned for 0.2 by the user on 2026-11-09.

**What it is.** A `Deque<T>` in `std::collections`, a growable ring buffer: `push_back`,
`push_front`, `pop_back`, `pop_front`, `front`, `back`, indexing, `len`, `clear`, and
iteration from front to back.

```dz
let mut frontier = Deque<Cell>()
frontier.push_back(start)
while true {                                       // a breadth-first search
    let c = frontier.pop_front() or break
    ...
}
```

Today the choice is a `List` with `remove(0)`, which moves every element each time, or a
ring buffer written by hand.

**Why it matters.** Breadth-first search and flood fill, input and event queues, undo
history with a cap, and a frame-time average over the last N frames are all a queue, and
they are common in games.

**What it would take.** `std/collections/deque.dz`, listed in `STD_SOURCES`
(`crates/dz_project/src/lib.rs`), with `tie @items` over its buffer and `tie [*]` over its
elements, killed as `List`'s are (§6.7's table: a push may grow, a pop takes an element out).
Indexing wraps round the ring, so `tie [*]` cannot name a plain `m_data[*]` range; check
what §6.7 lets a ring buffer say, and bring it to the user if it needs a new form. A
`tests/run/deque` program covering wrap-around and growth while wrapped.
