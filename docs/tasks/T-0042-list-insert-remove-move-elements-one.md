---
id: T-0042
title: `List::insert` and `remove` move elements one at a time
milestone: M12
status: done
done: r67, 2026-10-21
kind: deferral
area: stdlib
where: `dz_project/src/std/collections/list.dz`, `insert` and `remove`
---

**What happens now.** Inserting at or removing from the middle of a `List<T>` has to shift
the tail by one slot. `list.dz` does that with a loop that **moves one element at a time**,
calling `T`'s move for each:

```dz
list.insert(0, x)       // n moves of T, one per element after the insertion point
list.remove(0)          // the same, in the other direction
```

That is the only correct way to do it for a `T` with a `Drop` or an `own` field, whose move
is real work. For the overwhelmingly common case — a `T` whose bytes can simply be relocated,
which is every plain struct of numbers — it is n individual element moves where one
`memmove` would do, and the C++ compiler cannot collapse it because each iteration goes
through a move it cannot prove is trivial.

**Why it matters.** `remove(0)` on a list is already O(n) and that is understood; the cost
here is the constant factor on top, which for a `List<Vec2>` of a few thousand is the
difference between one wide copy and a few thousand two-float moves. Game code removes from
the middle of lists constantly — despawning an entity, retiring a particle — so this is a
real per-frame cost rather than a cold-path one.

**Why it was deferred.** There is **no `memmove` builtin** yet. §14.3 gives three placement
primitives — `construct`, `destroy` and a move — which is what container authors need for
the general case, and nothing that relocates a run of bytes.

**What it would take.** A fourth primitive in §14.3 alongside the other three, plus a way to
ask whether `T` may be relocated by copying its bytes. The second is the interesting part:
Dazzle's rule is that a type with no `drop` and no `own` field is copyable (§6.3), and
"copyable" is very close to "byte-relocatable" but not identical — a type with a hand-written
`impl Copy` (§6.6) is copyable precisely *because* copying it is not a byte copy. So the
predicate wanted is "has no `Drop`, no `own` field, and no hand-written `Copy`", which the
type table can answer, and `insert`/`remove` would branch on it at compile time with
`#if` (§12.10). Then the fast path is one call and the slow path is the loop that exists.

Adding a primitive to §14.3 is a language change: a §21 revision row, and a `tests/run` case
covering both paths with a `Drop` type proving the loop still runs for it.

**Done in r67** (D-0081). `memmove` is a placement primitive beside `memcpy` (§14.3), and `List` shifts with it whenever `T::is_trivially_copyable` — the predicate `grow_to` already used for `realloc` — through `shift_up`/`shift_down`, which `insert`, `remove`, `remove_range` and `insert_slice` share. `tests/run/collections` runs both paths, a `Drop` element proving the loop still runs and drops each element once.
