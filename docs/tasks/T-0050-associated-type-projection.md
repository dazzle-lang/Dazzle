---
id: T-0050
title: Associated-type projection
milestone: M12
status: done
done: r75, 2026-10-23
kind: deferral
area: checker
where: `dz_check/src/check.rs`, `iterate_item`
---

**What projection is.** A trait may declare an *associated type* — a type each implementer
chooses:

```dz
pub trait Iterate {
    type Item
    fn next(&mut self) -> Item?
}
```

Projecting it means **naming it off a bounded type parameter**: in a function generic over
`T: Iterate`, writing `T::Item` for whatever that particular `T` chose. The checker resolves
an associated type *through a known implementer* (`SliceIter<i32>`'s `Item` is `i32`) but
cannot name it off a bound, so `T::Item` has no meaning in a generic context.

**What that blocks.** Three things, and they are not small:

```dz
fn count<T: Iterate>(mut it: T) -> usize {
    for x in it { … }                    // DZ0801: the loop's element is T::Item
}
```

- **`for` over a generic `T: Iterate`** is refused (`DZ0801`) — the loop's element type *is*
  the projection. Concrete containers loop fine (r47).
- **`for` over a generic `C: Iterable`** is refused for the same reason, one level deeper:
  the element is the projection of `C::Iter`'s `Item`
  ([T-0040](T-0040-for-over-generic-c-iterable.md)).
- **Real iterator adapters** cannot be written at all: `indexed()`, `filter`, `map`, `rev`,
  `zip` are each a struct wrapping another iterator whose own `Item` is built from the inner
  one's, which is exactly a projection. This is why `indexed()` is a `for`-head special case
  rather than a type ([T-0037](T-0037-indexed-exists-only-in-for-head.md)).
- **Generic arithmetic** cannot name its result type — `fn add<T: Add>(a: T, b: T) -> T::Output`.

**Why a game needs it** (from the readiness review, 2026-09-14). Not for any one feature,
but because it is the single blocker under four of them. Any generic code that *consumes* a
sequence rather than a concrete container needs it, and that is most of what a reusable
systems layer in a game is: a function that takes anything iterable and does something per
element.

**What it would take.** A `Ty::Assoc { on: Box<Ty>, name }` form that the type table can
carry unresolved, plus normalisation: when `on` becomes concrete, look the implementation up
and replace the projection with what it chose; when it stays a parameter, two projections of
the same parameter and name are the same type and otherwise nothing is known about it.
`iterate_item` is where the current resolve-through-an-implementer path lives and is the
place to start. Equality, inference and the bound checker all have to learn the new form, so
this is the largest of the M12 checker rows — and the one that pays for itself most, since
it closes [T-0037](T-0037-indexed-exists-only-in-for-head.md),
[T-0040](T-0040-for-over-generic-c-iterable.md) and part of
[T-0049](T-0049-trait-objects-that-are-not-object.md) behind it.

**Where it is** (r75). As sketched above: `Ty::Assoc(Proj)` in `dz_hir/src/ty.rs`, carrying
the base, the trait with its arguments, and the name. `T::Item` written in a type is
`Ctx::project_named`. A trait's method read through a bound gets projections for its
associated types (`TypeTable::assoc_bindings`). `TypeTable::normalize` replaces a projection
once its base is concrete, and every substitution in the checker, `dz_mono` and `dz_cpp`
goes through `TypeTable::subst`, which ends with it. What a projection may do is
`TypeTable::abstract_bounds`. New codes: `DZ0560` (no bound declares the name) and `DZ0561`
(two bounds do). Tested by `tests/run/projection`, a case each in `typed/` and
`typeerrors/`, six files in `tests/errors/`, and a construct and a mistake in the feature
matrix. [T-0040](T-0040-for-over-generic-c-iterable.md) closed with it. A real `indexed()`
([T-0037](T-0037-indexed-exists-only-in-for-head.md)) can now be written, and is still open.
