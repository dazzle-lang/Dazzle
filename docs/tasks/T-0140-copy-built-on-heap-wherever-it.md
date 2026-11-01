---
id: T-0140
title: A copy is built on the heap wherever it is placed
milestone: none
status: open
kind: feature
area: ownership
---

Split out of [T-0117](T-0117-memory-placed-by-compiler-regions-result.md), stage 2,
round 3 ("`copy` is not done"), which left it for after the gate.

**Where the code is**

| Piece | Where |
|---|---|
| The copies, and the guard that sends a region value's copy to the heap | `List::copy` (`std/collections/list.dz`), `HashMap::copy` (`std/collections/hash_map.dz`); `Allocator::scoped` (`std/memory/allocator.dz`) |
| Which functions take a placement, and the sites | `result_functions` and the proposer in `dz_flow/src/placement.rs` |
| The C++ copy constructor and copy assignment | struct emission in `dz_cpp/src/lib.rs` |

**What it is.** A copy of a value that owns memory is always built on the heap, even when
the copy could live in its scope's region:

```dz
let names = other.names     // a copy of a `List<String>`: on the heap today, even when
                            // `names` is only borrowed and could live in this scope's region
```

**Why it matters.** It matters for speed only, since copies are correct on the heap (stage
1, step 3). A `String` or `List` copied in a loop body is heap traffic a region could
remove.

**Why it is this way.** A copy is made by C++'s copy constructor and copy assignment, which
take no placement.

**What it would take.** A second, placed copy for each type that owns memory (`List`,
`HashMap`, and every struct with a `copy` or with fields that have one), emitted where a
copy is bound by `let`, so the proposer can treat a copy as a site. Compiler and std work
(`docs/log/`), after T-0117's verdict.
