---
id: T-0099
title: Allocators::Temp gives a growing container a malloc per resize past 64 KiB
milestone: M12
status: open
kind: feature
area: stdlib
where: `crates/dz_project/src/std/memory/temp_arena.dz`, `grow` and `realloc`
---

**What it is.** The temporary arena hands out memory from 64 KiB chunks. An allocation that
does not fit gets a chunk of exactly its own size from `malloc`, and the arena only reuses
memory at `reset()`. So a `List` or `HashMap` in `Allocators::Temp` that grows past 64 KiB
costs a `malloc` at every resize, and keeps every array it has outgrown until the reset:

```dz
let mut best = HashMap<Cell, i32>(0, Allocators::Temp)   // grows 8, 16, … 131072 slots
```

`tests/bench_programs/astar` (T-0090) measured it. Putting each search's heap and maps in
the arena made the program about 5% slower than the global heap, and raised peak memory
from 20 MiB to 26. `words`, with one large table and text, was faster in the arena (about
7%) but peaked at 71 MiB instead of 44.

**Why it matters.** The arena is what the spec recommends for per-frame and per-task
scratch (§14.2). Scratch is often a container whose size is not known in advance. If the
arena is slower than `malloc` exactly there, "use `Temp`" is advice that backfires.

**What it would take.** Library work, no language change. Two options, which can be
combined:

- **Grow the chunk size geometrically**, doubling it each time a chunk is added and keeping
  the largest across `reset()`. A program's steady state then fits in one chunk after the
  first frame.
- **Extend in place more often.** `realloc` already grows the last allocation where it
  lies, but a container's resize is usually not the last allocation by then.

Re-run `python tools/bench_programs.py` and the A/B in the T-0090 log entry afterwards.
