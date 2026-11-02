---
id: T-0139
title: Frame regions: one region per pass of an update loop
milestone: none
status: open
kind: design
area: ownership
---

The user's idea, noted in [T-0117](T-0117-memory-placed-by-compiler-regions-result.md) as a
design question for after stage 2's first round. It needs a decision file.

**Where the code is**

| Piece | Where |
|---|---|
| Sites, the proposer, the decisions and their reasons | `dz_flow/src/placement.rs` (`place_with`, `Decision`, `Decision::label`) |
| The check that refuses a wrong placement | `analyse_regions` in `dz_flow/src/ties/body.rs` (`region_site`, `region_end`) |
| The hidden result placement every call can pass | `RESULT_PLACEMENT`, `placement_arg` in `dz_cpp/src/lib.rs` |
| The region arena | `std/memory/scope_arena.dz`, `scope_pool.dz` |

**What it is.** One region per pass of the game's update loop, shared by the functions the
pass calls. A frame's scratch memory is then a pointer bump to allocate and one step to
release, with nothing written by hand. Today a game does this by hand, and nothing checks
it:

```dz
// today: chosen and released by hand; a value kept past `reset()` reads released memory
fn update(&mut self, dt: f32) {
    defer Allocators::Temp.reset()
    let visible = self.cull(&camera, Allocators::Temp)
    …
}

// with frame regions: nothing written. `visible` goes in the frame's region, and a value
// that would outlive the frame is refused by the ties check and goes to the heap
fn update(&mut self, dt: f32) {
    let visible = self.cull(&camera)
    …
}
```

Scope regions (T-0117) are per block, and stage 2 builds a returned value where its caller
keeps it. A frame region adds what neither reaches: values kept across calls within a
frame and dropped before it ends, as when one system builds data, hands it to another, and
both are done with it by the end of the frame.

**Why it matters.** The per-frame linear allocator is the most common allocation idiom in
game engines. Made automatic and checked, it is something a C++ engine cannot have. It is
also what [T-0119](T-0119-remove-allocators-from-ordinary-code.md) needs before
`Allocators::Temp` can leave ordinary code without losing speed: hand-placed `Temp` was 6–7%
ahead of what placement reached in T-0104.

**What it would take.** Four questions for the user:

1. **How the frame is found.** (a) Inferred: the body of the outermost loop in `main`.
   (b) Marked: an attribute on the loop, or on the function called each frame
   (`#[frame] fn update(…)`). (c) Any loop body whose calls take a result placement. (b) is
   explicit and cheap to check; (a) is magic.
2. **A value stored in a field but replaced every frame** (`self.visible = cull(…)`). It
   goes to the heap today. A frame region could hold it only if the check proves the field
   is reassigned or cleared before the frame ends, the question
   [T-0120](T-0120-owner-arenas-for-containers-cleared-all.md) asks for containers.
3. **How callees reach the region.** (a) Stage 2's hidden parameter, already on every
   function returning something that owns memory. (b) A per-thread "current frame" that
   callees allocate from: no parameter, but an implicit context of the kind §14.1 argued
   against.
4. **What the editor shows**: "region: frame of `update`".

A language change if (1) is an attribute: a decision file, §14.6, and a §21 row.

**Looked at 2026-11-02: not needed yet; agreed with the user to close the gaps first.** The
example above already works without a frame region. `let visible = self.cull(&camera)` in
`update` is in `update`'s region: scope regions open one per call, and result placement
carries it to `cull` through the hidden parameter. So the frame region the task describes is
the region of the function called each frame (or of the loop body), and question 3 is
answered by what exists, (a). What kept per-frame values on the heap was narrower:

| Gap | Where it went |
|---|---|
| `format!`'s text, built in the macro's lambda: every HUD line | fixed 2026-11-02, placed where the macro's value goes |
| a value handed by value to another system that only reads or drops it | fixed 2026-11-02 within a module (`Staying`); across modules, T-0142 |
| a field replaced every frame (question 2) | the same question as [T-0120](T-0120-owner-arenas-for-containers-cleared-all.md), and moved there |

What a frame marker would still add is a *policy*, not a place: a region reset and kept
between passes, so a loop that needs the same large blocks (16 KiB and up, the heap's today)
stops calling `malloc` once it is warm. T-0117 showed that this speed costs peak memory (it
was the pool's), the right trade for a game loop and the wrong one for any loop, so if it
comes it is (1)(b), `#[frame]` on the function, opt-in. It waits until
[T-0138](T-0138-game-ten-thousand-lines-test-ties.md)'s game shows per-frame `malloc` of
large blocks costs something.
