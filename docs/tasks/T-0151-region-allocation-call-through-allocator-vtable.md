---
id: T-0151
title: A region's allocation is a call through the Allocator vtable, so its pointer bump is never inlined
milestone: M15
status: done
kind: feature
area: codegen
---

A container holds its allocator as `^mut Allocator`, a trait object, so every allocation,
reallocation and free was `vt->alloc(data, …)`: an indirect call the C++ compiler cannot
inline. For a region (spec §14.6, T-0117) that call wrapped a pointer bump, so the region
saved only the inside of `malloc`, not the call around it. Found while asking why regions
were only 3–14% faster than the heap after T-0117's verdict.

**Where the code is**

| Piece | Where |
|---|---|
| The trait object's methods: with regions on, `Allocator__dyn`'s check for the region arena's vtable and call its implementation directly (`trait_object_bodies`, `region_allocator`), written after `impl_vtables` so both are declared | `dz_cpp/src/lib.rs` |
| The arena's `alloc`, cut to the pointer bump so it inlines; everything else in `alloc_slow` | `dz_project/src/std/memory/scope_arena.dz` |

**Done 2026-11-03.** With regions on, a call through an `Allocator` handle first compares
the handle's vtable with `ScopeArena`'s and, when they match, calls `ScopeArena`'s
implementation directly; any other allocator goes through the vtable as before. With
regions off nothing is emitted differently but the order of the handles' member
definitions. `std`'s containers are compiled in the same translation unit as the arena, so
there the bump is inlined; a call from the program's own file is a direct call, not
inlined.

**Measured** with the compiler before and after, both building with `DZ_REGIONS=1`, `--release`,
twenty alternating runs, on a Ryzen 7 H 255 (16 threads), clang 23 and g++ 16. The
machine was shared, so the fastest runs are the figure to read:

| Program | clang++, fastest after / before | g++, fastest after / before |
|---|---:|---:|
| astar | 0.959 | 1.009 |
| words | 0.984 | 1.010 |
| expr | 0.939 | 0.922 |

So the indirect call was a small part of what an allocation costs here: 2–8% where it
helps, and about 1% slower on g++ for `astar` and `words`, consistent across two runs,
which the extra compare on every call may explain. Most of what keeps regions near the
heap is elsewhere: drops that still run (T-0147), allocations the placement does not reach
(T-0146, T-0150, T-0141, T-0142), and costs that are not allocation (T-0096, T-0097).
