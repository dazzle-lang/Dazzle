---
id: T-0124
title: Two `Rc`s may hold one value, and the ties check takes them for two
milestone: none
status: open
kind: design
area: ownership
---

Point 2 of [D-0094](../decisions/D-0094-safe-code-has-no-undefined-behaviour.md), which is
open.

**Where the code is**

| Piece | Where |
|---|---|
| What a guard binds: `or`, `if let` and the guard local | `dz_flow/src/ties/body.rs`: `hir::ExprKind::Guarded` (~2205), `IfLet` (~1994), `Or` (~2038), `unwrapped` |
| The handles | `std/memory/rc.dz`, `weak.dz`, `rc_block.dz`, `nullable.dz` |
| The call check the fix relies on | [T-0123](T-0123-call-may-reach-memory-it-kills.md) |

**What it is.** A guard on an `Rc` holds a count, so the value cannot be freed while it is
referred to (D-0062). Nothing tells the ties check that two handles share one value, so to
the check they are two unrelated locals. This is accepted, and corrupts as
[T-0123](T-0123-call-may-reach-memory-it-kills.md)'s program does:

```dz
let mut a = Rc<List<String>>(move(list))   // a full list
let b = a                                   // one list, two handles
if let &mut dst = a {
    if let &src = b {
        append_copy(&mut dst, &src)         // accepted: 7 of 8 copies read from freed memory
    }
}
```

**Why it matters.** Shared assets and entities are what games keep in `Rc` (§14.5's
texture), so any function given two of them can hit this.

**Why it is this way.** D-0062 settled what happens when a handle is emptied while its
value is in use. It did not settle two handles changing one value.

**What it would take.** A choice:

- **A. One place per `T`** (proposed). A guard on any `Rc<T>` or `Weak<T>` gives a
  reference into a single place no local holds, standing for "a value some `Rc<T>`
  shares". T-0123 then refuses the call above, and the kill check refuses a view through
  one handle used after a kill through another. There is no run-time cost. The false
  positive is a view into one `Rc<Texture>`'s value kept across a kill through a
  *different* `Rc<Texture>`: rare, and fixed by copying out what is kept.
- **B. A borrow flag, checked at run time**, as Rust's `RefCell` does. `if let &mut` sets
  the flag for the guard's life, and another guard on the same value panics while it is
  set. It costs a byte and a compare per guard, beside the count the guard already
  changes, and it catches only what runs.
- **C. `Rc` gives `&T` only**, and changing a shared value needs a cell type. That is
  Rust's model, and it costs the ergonomics §14.5 chose.

A language change: §14.5 and §6.7, and a §21 row.
