---
id: T-0210
title: A static that owns memory reports leaks and a bad free under --track-memory
milestone: none
status: done
kind: bug
area: codegen
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

**Done 2026-11-13.** `dz_drop_all_statics__` already dropped the root module's statics before
`std`'s, the tracker last. But `KEPT` was not among them: `in_cell` asked `needs_drop`, which
says no for any `T?`, so `KEPT` was a plain C++ static, destroyed after the `atexit` hook had
dropped the tracker. `in_cell` now looks inside an optional or fallible type, and `transfers`
is in the tracked runs. See `docs/log/m12.md`.

**Why it matters.** Any program with a static that owns memory cannot be checked with
`--track-memory`, which is why `transfers`, the program that moves strings between lists the
most, is not in `a_tracked_build_reports_leaks_and_bad_frees`.

**Where the code is.**

| Piece | Where |
|---|---|
| The tracker and its report | `crates/dz_project/src/std/memory/tracker.dz` |
| Dropping statics at exit, `dz_drop_all_statics__` | `crates/dz_cpp/src/lib.rs` (`drop_all_statics`) |
| Which statics are dropped there: `in_cell` | `crates/dz_cpp/src/lib.rs` |
| The tracked runs, `transfers` among them | `a_tracked_build_reports_leaks_and_bad_frees` in `crates/dz/tests/run.rs` |
