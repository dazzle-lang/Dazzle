---
id: T-0125
title: A `Box` emptied while `if let` holds its value
milestone: none
status: done
kind: design
area: ownership
---

Point 3 of [D-0094](../decisions/D-0094-safe-code-has-no-undefined-behaviour.md), which is
open.

**Where the code is**

| Piece | Where |
|---|---|
| `Box`: its pointer field, and its guard, which holds nothing | `std/memory/box.dz` (`m_ptr: own ^mut T`), `std/memory/unguarded.dz` |
| What a guard binds | `dz_flow/src/ties/body.rs`: `Guarded` (~2205), `IfLet` (~1994), `Or` (~2038) |
| A `Box`'s contents in placement (T-0117 stage 2, round 3) | `contents` in `dz_flow/src/ties/body.rs` |
| The rule being revised | D-0062; §14.5, the paragraph "A `Box` has no second owner to take…" |

**What it is.** A `Box` has one owner, so its guard takes nothing, and D-0062 says a `Box`
"can dangle as a `^T` can". This is accepted, and reads freed memory:

```dz
struct Holder {
    pub node: Box<Node> = null
}

if let &n = h.node {
    h.node = null                   // frees the node
    println!("{}", n.name)          // accepted: reads freed memory
}
```

**Why it matters.** It is a hole in safe code that ties can close exactly. D-0062 was
decided before ties existed, when nothing could follow a reference to what a `Box` owns.

**Why it is this way.** The guard's reference points at the inline handle, `h.node`, rather
than at the memory the handle owns. Writing `h.node` kills only what is under a pointer
(`Reach::Owned`), so `n` survives the write.

**What it would take.** Give `Box` a tie, `tie @value = m_ptr`, and make a guard's
reference a view in `h.node@value`. Then `h.node = null`, assigning another `Box`, and
`move(h.node)` all kill `n`, as assigning a `String` kills a view of its old text. There is
no run-time cost, and only programs like the one above are refused. Placement's `contents`
should use the same tie.

A language change: revises D-0062 and §14.5, with a §21 row.

**Built 2026-11-02 (r103).** `Box` declares `tie @value = m_ptr[*]` (`std/memory/box.dz`):
`m_ptr[*]` rather than `m_ptr`, so a path can go on into the value
(`kills h.node@value.name@text`; a tie ending at a pointer cannot). In the walk's
`Guarded` case, a guard of a `Box` named by a place gets a view as `&xs[i]` does
(`box_handle`): its target is `@value`, its tie the allocation, which a write, `null` or
a move of the box ends. A box that is a temporary keeps the old view of its guard. The
report shows the allocation as `@value` too. Not done: placement's `contents` does not
use the tie yet. Tests: `tests/flow/box_value_tie.dz`.
