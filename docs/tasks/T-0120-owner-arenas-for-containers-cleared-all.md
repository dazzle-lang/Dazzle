---
id: T-0120
title: Owner arenas for containers cleared all at once
milestone: M22
status: done
kind: deferral
area: ownership
---

Step 7 of D-0091's plan, split out of [T-0107](T-0107-design-ties-kills-clauses-compiler-placed.md);
cut from the first version of placement ([T-0117](T-0117-memory-placed-by-compiler-regions-result.md)).

**What it is.** The contents of a container cleared only all at once — frame data in a
private `self.scratch`, cleared every frame — could live in an arena the container owns,
reset when it is cleared, instead of on the heap.

**Why it matters.** Per-frame scratch data is common in games, and an arena per container
would make it free to build and to clear. On `frame` (below), patched by hand, it takes
0.71× the time of the heap on clang++ and 0.80× on g++ (2026-11-18).

**Where the code is**

| Piece | Where |
|---|---|
| The benchmark, both halves | `tests/bench_programs/frame/` (`src/main.dz`, `main.cpp`) |
| Which fields keep an arena, and which methods push into them | `owner_arenas`, `OwnerArenas` in `crates/dz_flow/src/arenas.rs` |
| The caller's side: a value stored into the owner goes to its arena, and why not | `arena_target`, `or_arena`, `Target::Arena`, `Decision::Arena` in `crates/dz_flow/src/placement.rs` |
| The check: an arena site goes only into its container, never assigned, made on any pass | `arena_of` in `Regions`, `region_site`, `region_store`, `region_assign` in `crates/dz_flow/src/ties/body.rs` |
| The arena: lives in memory from the owner's allocator, chunks kept on reset | `crates/dz_project/src/std/memory/owner_arena.dz` |
| The hidden C++ member, made on first use, moved with the field, destroyed after it | `ArenaSlot` in `crates/dz_cpp/src/runtime.rs`; emitted in `struct_decl` and `move_members` |
| The reset after `clear`, and the placement passed at a call | `arena_cleared`, `placement_arg` (`Placed::Arena`) in `crates/dz_cpp/src/lib.rs` |
| Tests | `a_value_stored_into_a_list_emptied_all_at_once_goes_to_its_owners_arena` in `crates/dz_flow/tests/placement.rs`; `tests/run/owner_arena` |
| Skipped drops | `skipped_drops` in `crates/dz_flow/src/release.rs` (T-0118) |
| The arena a region is, its chunks and pool | `crates/dz_project/src/std/memory/scope_arena.dz`, `scope_pool.dz` |
| How a container reads where it was placed, and `clear` | `#placement` in `List`'s constructor and `clear` in `crates/dz_project/src/std/collections/list.dz` |

**Why it is this way.** Every container in `examples/` and the other benchmarks holds plain
data (`Brick`, `Thing`, `Vertex`, `Object`, `Cell`, `u8`), with no owned contents to place,
so until `frame` (below) nothing exercised the rule for when an arena is safe (cleared only
all at once, a private field or a local).

**What it would take.** A real program with a container of owning values cleared each
frame (`frame`, written), measured against the heap (done, below); then the rule, checked by
ties.

**From [T-0139](T-0139-frame-regions-one-region-per-pass.md), 2026-11-02:** a field
*replaced* every frame (`self.visible = self.cull(…)`) is the same question. It goes to the
heap today ("only a `let` gets a region"). The old value is dropped during the next frame,
so a region holding it must outlive the frame that made it. That means either proving the
field is cleared before its frame ends, or two regions used in alternate frames, with proof
that the field is reassigned on every pass.

## 2026-11-18: the program

Moved into 0.3 (M22) by the user. The program the task asked for is
`tests/bench_programs/frame`: 10,000 units in a world a camera pans across, and a
`Renderer` whose private draw list, `List<Command>`, is emptied by `begin` and refilled
every frame with one command per unit in view (about 5,000), each owning a `String` label
and a `List<Point>` trail of two to eight points. 600 frames, about 3 million commands, 6
million allocations that live one frame. `dz placements` gives the case its reason:

```
main.dz:166:29: List<Point>  heap: stored in `renderer` on every pass of a loop
main.dz:170:25: String       heap: stored in `renderer` on every pass of a loop
```

The timings were left to the Linux machine, whose numbers are steadier than the Windows
one's; they are below. A first look on a busy Windows machine had said the arena is worth
enough to go on with; those numbers are not kept.

**How to measure what an arena would gain, before building it.** Three builds of the
generated C++ (`dz cpp tests/bench_programs/frame`, then `build/cpp/*.cpp` compiled by hand
with the flags `dz build --release` uses, which `tools/bench_programs.py` prints), timed
interleaved against the C++ half:

1. **heap**: the generated C++ as it is.
2. **arena**: in `frame.cpp`, a `::dz::std_::ScopeArena g_frame_arena{};` at namespace scope;
   the trail's constructor and the label's `String` (inside `format!`'s lambda) given
   `::dz::rt::addr(g_frame_arena)` in place of `::dz::std_::Allocators::Global()`; and in
   `Renderer::begin`, after `clear()`, the arena dropped and made again
   (`g_frame_arena.~ScopeArena(); ::new (&g_frame_arena) ::dz::std_::ScopeArena{};`), which
   gives its chunks back to the thread's pool for the next frame to take, so a warm frame
   calls no `malloc`.
3. **skipped**: as 2, with `clear()` replaced by `m_commands.m_len = 0;`, the drops skipped
   as T-0118's rule would.

All three must print the C++ half's checksum. Worth recording with them: peak memory, and
both compilers. C++ gets the same gain only if asked
(`std::pmr::monotonic_buffer_resource` and `pmr` containers through the program); Dazzle
would choose it with nothing written, which is the point of §14.6.

## 2026-11-18: measured, on the Linux machine

An arena is worth building: **the arena build takes 0.71× the heap build's time on clang++
and 0.80× on g++, and puts `frame` ahead of its C++ half on both (0.74× and 0.95×), where it
is behind today (1.04× and 1.19×).** Skipping the drops as well is a further 4–6%. Peak
memory does not move (4.5–4.6 MiB, against 4.7 for C++): a frame's commands are a few
hundred KiB, and the arena's chunks are reused frame after frame from the pool.

Ryzen 7 H 255, Debian clang 19.1.7 and GCC 14.2, at 1edd4ba; 15 runs of each build,
interleaved four ways, load average 0.3 before and 0.6 after:

| Compiler | Build | Median | Min | Against C++ | Against heap | Peak memory |
|---|---|---:|---:|---:|---:|---:|
| clang++ | heap | 289 ms | 285 ms | 1.04× | 1.00× | 4.5 MiB |
| clang++ | arena | 206 ms | 204 ms | 0.74× | 0.71× | 4.6 MiB |
| clang++ | skipped | 194 ms | 193 ms | 0.70× | 0.67× | 4.6 MiB |
| clang++ | C++ half | 277 ms | 274 ms | 1.00× | 0.96× | 4.7 MiB |
| g++ | heap | 326 ms | 324 ms | 1.19× | 1.00× | 4.6 MiB |
| g++ | arena | 261 ms | 260 ms | 0.95× | 0.80× | 4.6 MiB |
| g++ | skipped | 249 ms | 247 ms | 0.91× | 0.76× | 4.6 MiB |
| g++ | C++ half | 275 ms | 272 ms | 1.00× | 0.84× | 4.7 MiB |

Instructions, counted by callgrind, which load does not change, say the same:

| Compiler | heap | arena | skipped | C++ half |
|---|---:|---:|---:|---:|
| clang++ | 4.07 G | 3.09 G (0.76×) | 2.93 G (0.72×) | 3.77 G |
| g++ | 4.31 G | 3.31 G (0.77×) | 3.19 G (0.74×) | 3.86 G |

The time saved is a little more than the instructions saved, as expected of `malloc` and
`free`, whose cost is partly cache misses: in the heap build `free` alone is 8% of the
instructions on clang++.

All eight print the same checksum (`699681724039665181`). `tools/bench_programs.py frame`
in the same sitting gave 287 and 327 ms for the heap build, so the patched builds are
compiled as `dz build --release` compiles: they were built with its own commands, captured
by a `DZ_CXX` wrapper that logged them, with only `frame.cpp` changed and the objects linked
in the same order. One difference: g++ 14 crashed (an internal compiler error) loading
`dz`'s precompiled header from outside `dz`, so for g++ the patched `frame.cpp` includes
`std.h` itself, which changes no generated code. The arena is a `ScopeArena` in a union at
namespace scope, never destroyed: destroyed at exit it would give its chunks to the
thread's pool after the pool is gone.

What the numbers say for the design: the gain is the allocator, not the drops. The
arena build still runs every `~String` and `~List` and calls `dealloc` on each, and is most
of the way there; skipping the drops (T-0118's rule, which needs nothing in the elements to
release) is the smaller step. So the rule can come first and skipped drops after, measured
again.

## 2026-11-18: built, for the case `frame` is

**What the user asked for**, in their words: the label and trail are first placed in the
loop's region, then, since they move into the renderer, placed directly in the commands'
region instead; that region lives in `main`'s, where the renderer is. And since an `if` may
move a value on only one branch, the value goes straight to the commands' region, the
biggest scope that can use it. The user set aside the general questions (an arena per
container with many small owners, sizing, frame regions shared by several containers) to
come back to only if this is slow. It is not:

| `frame`, 15 runs | heap (before) | owner arena | the hand patch, for comparison |
|---|---:|---:|---:|
| clang++, against C++ | 1.04× | **0.76×** (210 ms / 276) | 0.74× |
| g++, against C++ | 1.19× | **0.96×** (265 ms / 277) | 0.95× |

Peak memory 4.6 MiB, as before (C++: 4.7). Same machine and compilers as above, load
average 1.3–1.6. Within 2–3% of the hand patch, which used the arena directly: reading the
renderer's arena at run time, and making it the first time, costs about that.

**The rule as built** (`dz_flow/src/arenas.rs`):

- **Which fields.** A private field of a struct that is not generic, of type `List<E>` with
  `E` owning memory, whose struct's own bodies (the only ones that can reach it) use it only
  by `&self` methods, a shared borrow, `push` and `clear`. `pop`, `remove`, `truncate`, an
  index, `&mut`, a move or an assignment of the field disqualify it. A body that clears it
  takes no by-value parameter that owns memory (which could be in the arena); a body that
  pushes a parameter into it neither clears it nor uses `self` but to reach a field.
- **Which values.** One the caller stores into the owner, a local of the body whose struct
  is the caller's module's, through a method that pushes that parameter, on any path.
  Nothing between building it and the end of its block may use the owner but that call, and
  the owner is never lent `&mut`: either could clear it and reset the arena under the value.
  `dz placements` says so when that is why ("stored in `r`, used before it is: clearing it
  there would reset the arena it keeps").
- **The check** (`analyse_regions`) is told each arena site's container: the value may be
  stored only there, assigned nowhere, returned never, and may be made on every pass of a
  loop, since the reset bounds it.
- **The arena** (`std`'s `OwnerArena`) lives in memory from the allocator the field's list
  keeps, which its chunks come from too, so it never moves while the values in it keep its
  address. A reset rewinds to the first chunk and keeps them all, since a region takes
  nothing back until it ends. Chunks start at 4 KiB and double. A large block (16 KiB or
  more) is its own `malloc`, freed at once, as in a region.
- **In C++**, a hidden `::dz::rt::ArenaSlot` member just before the field, made on first
  use, destroyed after the field's elements, moved with them, and empty in a copy; `clear()`
  on the field is followed by the arena's reset.

Tested by `a_value_stored_into_a_list_emptied_all_at_once_goes_to_its_owners_arena` (an
arena every pass and on some passes; the heap, with its reason, when the owner is used in
between or lent, and no arena for a type that pops, clears in the pushing method, or has a
public field) and `tests/run/owner_arena`, which prints what the all-heap build prints,
including after the owner is moved holding a frame, and is clean under the sanitizers and
memory tracking.

**Not covered yet**, each for when a program needs it: a caller in another module (it
would read the arena at run time, as a lent container's home is read; not decided), an
owner reached through a reference or a field path, a local container, `HashMap` and the
other containers, a generic owner, and skipping the elements' drops on reset (the further
4–6% measured above).

