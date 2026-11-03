---
id: T-0136
title: Writing a static is `unsafe`, so mutable global state is never checked
milestone: none
status: open
kind: design
area: ownership
---

Point 7 of [D-0094](../decisions/D-0094-safe-code-has-no-undefined-behaviour.md), decided
on 2026-11-03 (r107) as option A below: a `kills` clause may name a static. It revises
D-0046.

**Where the code is**

| Piece | Where |
|---|---|
| "Changing a `static` is `unsafe`" | `dz_check/src/check.rs` (~8547); D-0046; §7.7 |
| Where a tie path starts: `self` or a parameter, never a static | `Root` in `dz_check/src/ties.rs`; `Bindings` and `Path::bind` in `dz_flow/src/ties.rs` |

**What it is.** Reading a static is safe; changing one is `unsafe` (D-0046). A safe
function can change a static inside `unsafe`, and its caller can hold a reference into
that static across the call. This is accepted, and reads freed memory:

```dz
struct Scores {
    pub list: List<i32> = List<i32>()
}

static SCORES: Scores = Scores{}

fn add(v: i32) {                    // safe to call
    unsafe { SCORES.list.push(v) }
}

add(41)
let top = &SCORES.list[0]           // reading a static is safe
for i in 0..100 { add(i) }          // moves the array `top` points into
println!("{}", top)                 // accepted: prints 41, from freed memory
```

The `unsafe` in `add` is where the fault is, by §1.1's rule. But no `unsafe` block can be
correct here, since it cannot know who holds a reference. A mutable static that owns memory
cannot be used soundly at all.

**Why it matters.** It matters little today: a static holds constants only (§7.7), and
std's uses (the temporary arena, the tracker) sit behind safe interfaces that hand out no
references into them. It will matter if global game state lives in statics.

**What it would take.** A choice:

- **A. A `kills` clause may name a static** (proposed): `fn add(v: i32) kills
  SCORES.list@items`. A write that its clause covers becomes safe. Tie paths gain a root
  for a static, and every caller's references into it are checked like any other.
- **B. A reference into a static that owns memory needs `unsafe`.** The hole is closed and
  the statics stay unchecked.
- **C. Keep D-0046**, and document the hole.

A language change: §7.7 and §6.7, and a §21 row.
