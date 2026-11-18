---
id: T-0120
title: Owner arenas for containers cleared all at once
milestone: M22
status: open
kind: deferral
area: ownership
---

Step 7 of D-0091's plan, split out of [T-0107](T-0107-design-ties-kills-clauses-compiler-placed.md);
cut from the first version of placement ([T-0117](T-0117-memory-placed-by-compiler-regions-result.md)).

**What it is.** The contents of a container cleared only all at once — frame data in a
private `self.scratch`, cleared every frame — could live in an arena the container owns,
reset when it is cleared, instead of on the heap.

**Why it matters.** Per-frame scratch data is common in games, and an arena per container
would make it free to build and to clear. How much it gains is not measured yet: `frame`
(below) is the program to measure it on.

**Where the code is**

| Piece | Where |
|---|---|
| The benchmark, both halves | `tests/bench_programs/frame/` (`src/main.dz`, `main.cpp`) |
| Each allocation's decision and its reason ("stored in `renderer` on every pass of a loop") | `Decision`, `Decision::label` in `crates/dz_flow/src/placement.rs` |
| The rule that sends a value pushed on every pass to the heap | the stored-values pass, `crates/dz_flow/src/storing.rs`; its tests are `drained` and `mapped` in `crates/dz_flow/tests/placement.rs` |
| The check that refuses a wrong placement | `analyse_regions` in `crates/dz_flow/src/ties/body.rs` |
| Skipped drops | `skipped_drops` in `crates/dz_flow/src/release.rs` (T-0118) |
| The arena a region is, its chunks and pool | `crates/dz_project/src/std/memory/scope_arena.dz`, `scope_pool.dz` |
| How a container reads where it was placed, and `clear` | `#placement` in `List`'s constructor and `clear` in `crates/dz_project/src/std/collections/list.dz` |

**Why it is this way.** Every container in `examples/` and the other benchmarks holds plain
data (`Brick`, `Thing`, `Vertex`, `Object`, `Cell`, `u8`), with no owned contents to place,
so until `frame` (below) nothing exercised the rule for when an arena is safe (cleared only
all at once, a private field or a local).

**What it would take.** A real program with a container of owning values cleared each
frame (`frame`, written), measured against the heap (open); then the rule, checked by ties.

**From [T-0139](T-0139-frame-regions-one-region-per-pass.md), 2026-11-02:** a field
*replaced* every frame (`self.visible = self.cull(…)`) is the same question. It goes to the
heap today ("only a `let` gets a region"). The old value is dropped during the next frame,
so a region holding it must outlive the frame that made it. That means either proving the
field is cleared before its frame ends, or two regions used in alternate frames, with proof
that the field is reassigned on every pass.

## 2026-11-18: the program; the measurement is open

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

**Not measured yet: the user runs it on the Linux machine**, whose numbers are steadier
than the Windows one's, so no timings are recorded here. A first look on a busy Windows
machine said the arena is worth enough to go on with; those numbers are not kept.

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

**Next**, by the plan put to the user: those numbers; then a decision file for the rule
(when a container may own an arena: emptied only all at once, private or local, nothing
taken out kept past a reset), where the arena lives (a hidden field beside the container, or
one in every container), and whether a reset keeps its chunks; then the implementation,
measured again on `frame`.

