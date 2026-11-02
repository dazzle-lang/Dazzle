---
id: T-0131
title: Memory tracking poisons every freed block, heap and region alike
milestone: none
status: done
kind: feature
area: stdlib
---

Part of the testing [D-0094](../decisions/D-0094-safe-code-has-no-undefined-behaviour.md)
calls for; it needs no decision.

**Where the code is**

| Piece | Where |
|---|---|
| The heap's `free`: recorded under tracking, not poisoned | `Heap::free` in `std/memory/heap.dz` (~47) |
| A region: `free`, `free_large`, `realloc` (the old block), `release`, `drop` | `std/memory/scope_arena.dz`; `std/memory/scope_pool.dz` |
| The one place that poisons today | `TempArena::reset`, `std/memory/temp_arena.dz`, through `poison` (`std/memory/poison.dz`) |

**What it is.** `--track-memory` records every heap allocation and catches a double free.
A block freed through `Allocators::Global` keeps its old bytes until the C heap reuses it,
so a read through a dangling reference sees plausible data. In the review behind D-0094,
`apart(&mut xs, &mut xs)` ([T-0123](T-0123-call-may-reach-memory-it-kills.md)) printed the
freed list's first element, `41`, correctly. Only `Temp.reset()` poisons what it releases,
with `0xDD`. A region poisons nothing: an outgrown array stays in its chunk, holding its
old contents, until the scope ends.

**Why it matters.** Every hole in D-0094 would have shown up as garbage instead of a
plausible value. It makes the corpus, the sanitizer job
([T-0132](T-0132-sanitizer-builds-in-ci-run-corpus.md)) and the fuzzer
([T-0134](T-0134-fuzz-ties-checker-against-sanitizer.md)) better at finding holes, and
regions need it most: a wrong placement is a read of released memory.

**What it would take.** Under `TRACK_MEMORY`, fill every freed block with `0xDD` before it
goes back. `free` is sized, so the size is known. A stale integer then reads `0xDDDDDDDD`,
and a stale `String` has a garbage length that a bounds check catches. Holding freed blocks
for a while before reuse (a quarantine, as AddressSanitizer does) would catch more, but is
optional. The cost falls on tracking builds only. Compiler and std work (`docs/log/`).

**Built 2026-11-02.** Under `TRACK_MEMORY`, through `poison`:

- `Heap::free` overwrites the block after the tracker accepts the free, before C gets it.
- `TempArena::free` overwrites the block, which stays in its chunk until `reset`.
- A region overwrites every small block it frees (all but the last stay in their chunk),
  the tail a shrink in place gives up, and every large block before it goes back to the
  heap; when it ends, everything its chunks held, before they go to the pool, and the
  large blocks it still holds. A large block's header is 32 bytes again, to keep its size.

No quarantine: a freed heap block may be reused at once. Tests:
`a_tracked_build_poisons_what_it_frees` in `dz/tests/run.rs`, which reads a region's freed
block, a region's chunk after the region and a `Temp` block, `0xDD` tracked and the old
bytes untracked. The heap is not read there, since what C's heap writes into a freed block
differs by platform. `tests/run/region_arena` no longer asserts that a freed large block
comes back at the same address, which since large blocks became the heap's was true only
by chance of the C heap.
