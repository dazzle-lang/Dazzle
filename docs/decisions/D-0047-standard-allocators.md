---
id: D-0047
title: The standard allocators
status: decided
---

**`Allocators::Global` and `Allocators::Temp`, no `Arena` for now; memory tracking is opt-in.** Global is `malloc`/`free`; Temp is a per-thread arena reset where the program chooses. The `Allocator` trait takes `&mut self` — an arena changes on every allocation — and its methods end in `loc: SourceLocation = #location`, which containers forward, so tracking reports name user code. Tracking is off in every build mode until `[build] track_memory = true` or `--track-memory` turns it on, and then reports leaks at exit, double and foreign frees, a free whose size or alignment differs from the allocation, and poisons Temp's memory on reset so a stale read shows. The allocator types are in the prelude; larger stdlib areas are imported modules (§14)
