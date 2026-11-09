---
id: T-0177
title: BitSet: a set of small integers in std
milestone: M12
status: open
kind: feature
area: stdlib
---

Planned for 0.2 by the user on 2026-11-09.

**What it is.** A growable `BitSet` in `std::collections`, one bit per integer, stored in a
`List<u64>`: `insert`, `remove`, `contains`, `len` (a population count), `clear`, iteration
over the set integers in order, and `union_with`, `intersect_with` and `difference_with`.

```dz
let mut visited = BitSet()
visited.insert(cell.index())
if !visited.contains(next.index()) { ... }
```

**Why it matters.** A `HashSet<usize>` or a `List<bool>` does the same job with 8 to 64
times the memory. Visited sets in grid searches, collision layers and masks, and which
components an entity has (the ECS benchmark,
[T-0172](T-0172-ecs-benchmark-against-rust-c-query.md)) all want one.

**What it would take.** `std/collections/bit_set.dz`, listed in `STD_SOURCES`
(`crates/dz_project/src/lib.rs`). Its elements are integers handed out by value, so it needs
no `tie [*]`; iteration ties to `@items`. Population count and trailing zeros come from the
C++ compiler's builtins through the runtime shim, on both clang++ and g++ (invariant 7), or
from a plain loop if the shim has none yet. A `tests/run/bit_set` program.
