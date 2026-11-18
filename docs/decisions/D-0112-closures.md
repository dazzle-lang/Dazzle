---
id: D-0112
title: Closures
status: decided
---

**Where it stands.** Opened on 2026-11-18, when the user, choosing how a parallel loop's
body is written (D-0096), answered "we need closures", and answered the same day (below,
"Agreed"). In the spec as §7.8, r144. Built as
[T-0259](../tasks/T-0259-closures-for-parallel-loop-bodies-first.md).

**Agreed, 2026-11-18:**

| | |
|---|---|
| **Syntax** | a function without a name, `fn(a, b) { … }`: `enemies.sort_by(fn(a, b) { return a.hp < b.hp })`. A parallel loop turned out not to need one: it became a `for` over a parallel iterator the same day (D-0096) |
| **Captures** | inferred from use, as Rust and Swift: reading `dt` captures `&dt`, writing it captures `&mut dt`; the editor shows them on hover |
| **In 0.3** | view closures only: they capture by reference, are passed down and called, and are never stored. Owning closures, for callbacks in fields and `spawn`, come later |
| **Their type** | one per signature, `view fn(p: &mut Position)`: a code pointer and an environment pointer, one C++ type, no generic instance per closure. A `kills` clause and `#[main_thread]` are part of it, as for a function type (§6.7, D-0096) |

The sketch below is what was decided from; where they differ, the table holds.

**Why now.** Dazzle has no closures, so `join!` and `for_each!` take named functions, and a
loop body that needs the locals around it becomes a function plus a context struct. A
parallel loop should read like a loop. The same gap is felt elsewhere: a comparator for a
sort, `map` and `filter` on iterators, a callback.

**What any design must respect.**

- **References are never stored** (D-0003), except in a view (D-0093), which is placed as a
  reference is. A closure that captures a reference is exactly that kind of value.
- **Ties** (§6.7): what a closure captures is tied, and a kill of it ends the closure.
- **Definition-site generics** (§8): a function taking a closure is checked once.
- **Threads** (D-0096): in `join!` and `for_each!`, captures are checked as the calls'
  arguments are; in `for_each!` every pass shares them, so a captured `&mut` is refused
  (D-0096 hole 4) unless it is the pass's own element.

**A sketch: two kinds, by how they capture.**

1. **A view closure** captures by reference. It is a view: it may be passed down and
   called, never stored in a field, returned past what it captures, or moved to a thread
   that outlives it. That is all `for_each!`, `join!`, `sort_by` and `map` need, and it
   costs no allocation.

   ```dz
   let dt = clock.delta()
   for_each!(w.query<&mut Position, &Velocity>(), |p, v| {
       p.x += v.x * dt              // `dt` captured as `&`
   })
   ```

2. **An owning closure** captures by value, moving what it takes. It owns its captures, so it
   may be stored (a callback in a field, a job for `spawn`) and is `Send` when everything it
   captured is.

**Open.**

- **The syntax**: `|a, b| { … }` as Rust, `fn(a, b) { … }` as a function without a name, or
  another.
- **How a capture is chosen**: inferred from use (Rust), or written (C++'s `[&x, y]`).
  Written suits Dazzle's explicit signatures; inferred is less to type.
- **The type of a closure**: a generic parameter (`F: Fn(&mut Position)`), instantiated per
  closure as Rust does; or one type per signature, `view fn(p: &mut Position)`, a pair of a
  code and an environment pointer, which is one C++ type and adds no instances.
- **`kills` on a closure**: its body's kills are part of its type, as a function type's
  clause is (§6.7).
- **Whether owning closures are in 0.3**, or only view closures, which are what threads need.
