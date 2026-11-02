---
id: T-0120
title: Owner arenas for containers cleared all at once
milestone: none
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
would make it free to build and to clear. How much it gains is unmeasured.

**Why it is this way.** Every container in `examples/` and the benchmarks holds plain data
(`Brick`, `Thing`, `Vertex`, `Object`, `Cell`, `u8`), with no owned contents to place, so the
rule for when an arena is safe (cleared only all at once, a private field or a local) cannot
be tested here.

**What it would take.** A real program with a container of owning values cleared each
frame, measured against the heap; then the rule, checked by ties.

**From [T-0139](T-0139-frame-regions-one-region-per-pass.md), 2026-11-02:** a field
*replaced* every frame (`self.visible = self.cull(…)`) is the same question. It goes to the
heap today ("only a `let` gets a region"). The old value is dropped during the next frame,
so a region holding it must outlive the frame that made it. That means either proving the
field is cleared before its frame ends, or two regions used in alternate frames, with proof
that the field is reassigned on every pass.
