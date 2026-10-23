---
id: T-0037
title: `indexed()` exists only in a `for` head
milestone: M12
status: open
kind: deferral
area: checker
where: `dz_check/src/check.rs`, `for_stmt`
---

**What `indexed()` is today.** It gives a loop the element *and* its position:

```dz
for (i, enemy) in enemies.indexed() { … }
```

But it is not a function, a method or an adapter — it is a **shape the checker recognises in
a `for` head** (`for_stmt`) and lowers to a counted loop. Nothing declares it. So it exists
in exactly one syntactic position, and anywhere else it is not a name:

```dz
let pairs = enemies.indexed()           // refused: there is no such method
for (i, e) in move(xs).indexed() { … }  // refused for the same reason
```

**Why it is this way.** A real `indexed()` is a generic struct wrapping another iterator and
implementing `Iterate`, whose `Item` is a pair built from the inner iterator's `Item`. Naming
that inner `Item` is **associated-type projection**, which the checker cannot do
([T-0050](T-0050-associated-type-projection.md)). So the adapter cannot be written in Dazzle
yet, and the `for`-head special case is what stands in for it.

**Why a game needs it** (from the readiness review, 2026-09-14). `indexed()` as a real
adapter is the first of a set — then `filter`, `map`, `rev`, `zip`. Iterating a list of
entities while skipping the dead ones, pairing two lists, or walking one backwards is
everyday game code, and today each is a hand-written `while` with an index. The special case
also costs in a subtler way: it is a name that works in one place and not another, which is
precisely the kind of rule that makes a language hard to predict.

**What it would take.** [T-0050](T-0050-associated-type-projection.md) first — it is the
blocker, and it unblocks this, `for` over a generic `C: Iterable`
([T-0040](T-0040-for-over-generic-c-iterable.md)) and generic arithmetic at the same time.
Then `indexed()` becomes an ordinary `std` struct in
`crates/dz_project/src/std/collections/`, the `for_stmt` special case is deleted, and a
`tests/run` case checks the two refused forms above now work. Deleting the special case is
part of the job: two implementations of the same name would drift.

**Unblocked** (r75): [T-0050](T-0050-associated-type-projection.md) is done, and an adapter
whose `Item` is built from the inner one's can now be written — `tests/run/projection` has
one, `Numbered<I>`, with `type Item = (usize, I::Item)`. What is left is the work above.
