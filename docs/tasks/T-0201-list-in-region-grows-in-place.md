---
id: T-0201
title: A list in a region grows in place beside what is built into its home
milestone: none
status: open
kind: feature
area: ownership
---

Left from [T-0200](T-0200-homes-built-sends-clauses-home-tie.md) stage 4, as a cost of
[D-0097](../decisions/D-0097-homes-ownership-that-travels-with-its.md)'s homes.

**Where the code is**

| Piece | Where |
|---|---|
| A list's growth, trying in place first | `List::move_to` in `std/collections/list.dz` |
| Growing in place, the last allocation only | `ScopeArena::resize_in_place` in `std/memory/scope_arena.dz` |
| The region arena's chunks and pool | `std/memory/scope_arena.dz`, `scope_pool.dz` |
| The frame loop it was measured on | D-0097, "A game's frame, measured" |

**What it is.** A `List` in a region grows where it is only while its buffer is the region's
last allocation (`resize_in_place`). Since homes, a function lent a list builds what it
pushes in the list's home, so on D-0097's frame loop the strings `combat` and `render` make
sit between the event list's and the draw list's growths. Each doubling then allocates afresh
and moves every element, leaving the old buffer in the region until it ends.

**Why it matters.** It costs the frame loop about 3%: 0.56 s, where the same loop with
those strings on the heap and the lists alone in the region ran in 0.54 s (the heap: 0.61 s).
Any container filled with values built for it grows this way.

**What it would take**, one of:

- **A region gives a growing block room.** Allocations above a size, or a block that has
  grown once, go to a second bump area of the region (its other end of the chunk, or a chunk
  of its own), where values built between growths do not land.
- **A list reserves on its first growth in a region** more than doubling, as a frame-scoped
  list will be refilled to the same size: a cheaper guess, worse for memory.
- **The compiler sizes it.** Where placement sees a list filled by a loop of known length,
  it passes a capacity: narrow, but free at run time.

Measure each against the frame loop and `words`, `expr` and `astar`.
