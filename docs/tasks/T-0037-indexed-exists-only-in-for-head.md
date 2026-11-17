---
id: T-0037
title: `indexed()` exists only in a `for` head
milestone: M12
status: done
kind: feature
area: checker
where: `crates/dz_project/src/std/collections/indexed.dz` (`Indexed<I>`); `crates/dz_project/src/std/core/iterate.dz` (`Iterable::indexed`); `dz_check/src/check.rs`, `for_stmt` (the loop head's counted form, kept) and `solve_call` (`Self` as a type argument); `dz_check/src/lib.rs` (an associated type's bounds inside its trait); `tests/run/indexed/`
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

**Done** (2026-11-17, r138), asked for by the user, but not quite as planned above: the
loop head's special case is kept. Two things stood in the way of deleting it.

- **A method has no by-value receiver** (§7.3), so `it.indexed()` on an iterator cannot take
  the iterator into the adapter. It could only copy it, which is right for a plain iterator
  and a silent copy of anything else. A default body that does that is accepted today, but
  only through a hole, T-0241. So `Iterate` has no `indexed()`: an iterator is numbered
  with **`Indexed(it)`**, which takes it by value.
- **`for i, &mut e in list.indexed()`** writes because the loop sees the `&mut` binding and
  asks for `iter_mut()`. A method cannot see how its result will be bound, so the adapter
  over a container is read-only.

What there is now: `std`'s **`Indexed<I>`** (`collections/indexed.dz`), an iterator of
`(usize, I::Item)`; **`Iterable::indexed()`**, a default method, `Indexed(self.iter())`, so
`let pairs = xs.indexed()` works on every container; and `Indexed(it)` for an iterator. A
loop head that names `indexed()` keeps its counted loop, with the same passes and bindings as
a loop over the value, plus `&mut e`, arrays and ranges, which spec §10.4 now says.

Two checker fixes came with it, both needed by `Iterable::indexed`'s body: `Self` written as
a type argument in a body is the enclosing type (`Indexed<Self>(…)`), and an associated type
used inside its own trait's default body has the bounds its declaration gives it (`type Iter :
Iterate` makes `Iter` an iterator there). A type argument that fails its bound is now the
error type afterwards, so `Indexed(xs)` of an array is one diagnostic, not also "cannot be
iterated" at the loop. Found on the way: T-0241, and T-0242 (a default body cannot reach a
supertrait's method through `self`). Tests: `tests/run/indexed`, `tests/errors/indexed_*`,
`typed/`'s `numbered`, and `dz_check`'s
`a_default_body_meets_bounds_through_self_and_its_associated_types`.

The adapters the readiness review wanted after this one (`filter`, `map`, `rev`, `zip`) meet
the same receiver question, which is a language decision of its own.
