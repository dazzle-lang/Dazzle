---
id: T-0210
title: A static that owns memory reports leaks and a bad free under --track-memory
milestone: none
status: open
kind: bug
area: stdlib
---

Found 2026-11-13 while widening the tracked-memory test (T-0208). `tests/run/transfers` run
with `--track-memory` prints its expected output, then reports two leaks and panics:

```
leak: 320 bytes allocated at …/transfers/src/main.dz:113
leak: 45 bytes allocated at …/transfers/src/main.dz:110
2 allocations leaked, 365 bytes
panic: free of a pointer Allocators::Global never allocated, at <std>/text/string.dz:301
  at Tracker::release (<std>/memory/tracker.dz:35:17)
```

Both allocations are in `static KEPT: List<String>?`, the list's buffer and a string pushed into
it. The same with the compiler before T-0208, so it is older.

**Why it happens (likely).** The tracker is a static of `std` that reports when it is dropped
(`tracker.dz`: "It reports leaks when the program ends, which is when this static is
dropped"). Statics are dropped at exit by `dz_drop_all_statics__`, registered with `atexit`
(`dz_cpp/src/lib.rs`). The tracker goes before the program's own `KEPT`: it reports what `KEPT`
still owns as leaked, and `KEPT`'s drop then frees into a tracker that is already gone. A
module's statics should be dropped before those of the modules it depends on, `std` last, as
C++ destroys in reverse order of construction.

**Why it matters.** Any program with a static that owns memory cannot be checked with
`--track-memory`, which is why `transfers`, the program that moves strings between lists the
most, is not in `a_tracked_build_reports_leaks_and_bad_frees`.

**Where the code is.**

| Piece | Where |
|---|---|
| The tracker and its report | `crates/dz_project/src/std/memory/tracker.dz` |
| Dropping statics at exit, `dz_drop_all_statics__` | `crates/dz_cpp/src/lib.rs` (around line 7498) |
| The tracked runs, which leave `transfers` out | `a_tracked_build_reports_leaks_and_bad_frees` in `crates/dz/tests/run.rs` |

**What it would take.** Drop statics in reverse dependency order, the tracker's module last;
then add `transfers` to the tracked runs.
