---
id: T-0079
title: Comparison operators do not read through a reference
milestone: M12
status: done
done: r68, 2026-10-21
kind: feature
area: checker
where: `dz_check/src/check.rs`, binary operators
---

**What is refused.** A `&T` reads as its `T` almost everywhere — `let v: i32 = r`, `r * 10`,
`r += 1`, passing it to an `i32` parameter — but not on either side of a comparison:

```dz
fn keep(x: &i32) -> bool {
    return x > 2                  // DZ0514: cannot compare `&i32` with `{integer}`
}
for k, &v in map {
    if v == k * 10 { … }          // DZ0514: cannot compare `&i32` with `i32`
}
let last = xs.last() or return
if last == 0 { … }                // the same
```

The workaround is a typed `let` first (`let n: i32 = x`), which the new `retain` callbacks
and `first()`/`last()` (r67) make common: a `fn(&T) -> bool` predicate compares its argument
on nearly every line. Found writing `tests/run/collections`, 2026-10-21.

**Why it matters.** It is the same rule applied two ways: arithmetic reads through, comparison
does not, and the help ("Dazzle has no implicit conversions — cast one side") suggests a cast
that cannot be written, since `x as i32` on a `&i32` is not a cast between numbers.

**What it would take.** In the checker's comparison operators, peel a reference off each
operand of a primitive type before unifying, as arithmetic already does; `&T == &T` through
`Eq` keeps working as it is. Probably a language clarification rather than a change — §5.2
says a reference reads as its value — but if it needs a sentence there it is a §21 row.
`typed/` gains the three forms above; `typeerrors/` keeps a comparison of a `&i32` with a
`str`.

**Done in r68.** `read_operand` in the checker reads a `&T` operand of a number, `bool`, `char`, `str` or enum as its value for comparisons, arithmetic and bitwise operators, whichever side it is on; struct references still go through `Eq`/`Ord`. Arithmetic had only worked when a type was wanted of the result, so `let y = x * 2` was refused too. §5.2 says so; `typed/` and `typeerrors/` have a case each, and `tests/run/collections` lost its workarounds.
