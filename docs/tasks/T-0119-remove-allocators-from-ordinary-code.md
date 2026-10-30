---
id: T-0119
title: Remove allocators from ordinary code
milestone: none
status: open
kind: feature
area: stdlib
---

Step 6 of D-0091's plan (spec §14.6), split out of
[T-0107](T-0107-design-ties-kills-clauses-compiler-placed.md); after
[T-0117](T-0117-memory-placed-by-compiler-regions-result.md).

**What it is.** With the compiler placing memory, std's allocator parameters, the examples'
and the benchmarks' `Allocators::Temp` and `#allocator` go. The `Allocator` trait,
`ScopeArena` and `TempArena` stay as `std` internals under `unsafe`.

```dz
let words = List<String>(Allocators::Temp)   // today
let words = List<String>()                   // after: placed by the compiler
```

**Why it matters.** It is D-0091's other half: no allocator to choose means none chosen
wrong, and ordinary code stops carrying allocator parameters.

**Why it is this way.** It cannot come before placement, or code would lose the speed hand
placement gives it now.

**What it would take.** std's container constructors and every caller, the examples, the
benchmarks, and the prototype's `STORES`/`TAKES` tables and `#allocator`. Measured against
hand-placed `Temp` as in T-0104: stage 3 reached `words`' keys but gained 2%, with
hand-placed `Temp` 6–7% ahead, which this must not lose.
