---
id: T-0143
title: Placement under a memory budget, or with no heap
milestone: M17
status: open
kind: design
area: ownership
---

§14.6: "A memory budget or a program with no heap is not expressible yet." It needs a
decision file.

**Where the code is**

| Piece | Where |
|---|---|
| Each allocation's decision, and why it went to the heap | `Decision` and `Decision::label` in `dz_flow/src/placement.rs` |
| What a target has (M17) | §19's M17 row; `#if` on capabilities (§12.10) |

**What it is.** With placement, the compiler already knows where every allocation goes, and
why. A function, a module or a whole target could then ask that nothing go to the heap, and
be told at each allocation that would:

```dz
#[no_heap]
fn mix(out: &mut []f32, history: &mut List<f32>) {   // an audio callback
    let scratch = List<f32>()       // fine: in the scope's region
    history.push(out[0])            // error: `history` grows on the heap
}
```

**Why it matters.** Embedded is one of Dazzle's targets, and M17 needs "no heap" to mean
something. Real-time game code, such as an audio callback or a physics step, wants "no
allocation here" guaranteed. C++ cannot check this; placement can.

**What it would take.** A choice of spelling:

- **A. An attribute on a function or a module** (`#[no_heap]`): every placement that would
  be the heap is an error, with the reason the editor already shows.
- **B. A build setting**, `[build] heap = false`, for M17's targets.
- **C. A budget per region** (`#[region_budget(64 KiB)]`), checked at compile time where
  sizes are known and at run time otherwise.

A language change: a decision file, §14.6, and a §21 row.
