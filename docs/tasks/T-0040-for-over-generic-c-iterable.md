---
id: T-0040
title: `for` over a generic `C: Iterable`
milestone: M12
status: done
done: r75, 2026-10-23
kind: deferral
area: checker
where: `dz_check/src/check.rs`, `container_iterator`
---

**What is refused.** A loop over a type parameter bounded by `Iterable` — the trait a
*container* implements, which hands out an iterator — reports `DZ0801`:

```dz
fn total<C: Iterable>(xs: &C) -> f32 {
    let mut sum = 0.0
    for x in xs { sum += x }        // DZ0801
    return sum
}
```

Looping over a **concrete** container works, and has since r47: `for x in &list`,
`for x in &map` and the rest are all fine.

**Why.** The loop's element type is `C::Iter`'s `Item` — an associated type reached through
*another* associated type — and the checker cannot name an associated type off a bounded
parameter at all ([T-0050](T-0050-associated-type-projection.md)). This is the same blocker
as `for` over a generic `T: Iterate`, one level deeper: `Iterate` needs one projection,
`Iterable` needs two.

**Why it matters.** It is the difference between a stdlib you can write generic helpers
against and one you can only use concretely. Any function that takes "something you can loop
over" — a sum, a search, a draw-all, a serialise-all — has to be written once per container
type today, or take a slice and so refuse anything that is not contiguous.

**What it would take.** Nothing of its own: it closes when
[T-0050](T-0050-associated-type-projection.md) does, and a `tests/run` case plus a
`typed/` case are the whole of the work on this row. It is listed separately because it is
the *symptom* a user hits — `container_iterator` is where the refusal is reported, and
someone reading that code should be pointed at the projection row rather than concluding the
loop lowering is at fault.

**Done** (r75), with [T-0050](T-0050-associated-type-projection.md): the refusal in
`container_iterator` is gone, and a loop over a `C: Iterable` binds a `C::Iter::Item`.
Tested by `each` and `total` in `tests/run/projection`, `count_items` in `typed/`, and
`count_of` in the feature matrix.
