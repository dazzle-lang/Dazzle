---
id: T-0051
title: An open-ended range held in a variable
milestone: M12
status: open
kind: deferral
area: codegen
where: `dz_cpp/src/lib.rs`, `ExprKind::Range`
---

**What is refused.** A range with both ends is a value: codegen emits the shim's
`dz::Range<T>{lo, hi, inclusive}`, which a `for` then counts through, and which can be
passed around and stored. An **open-ended** range — `lo..`, or `..hi` — can only be written
directly in a `for` head or a slice index:

```dz
for i in 5.. { … }                      // fine: the loop counts from 5
let tail = xs[3..]                      // fine: the slice knows the length

let r = 3..                             // DZ0801: "an open-ended range outside a `for` loop"
```

**Why.** `dz::Range<T>` holds two ends, so there is nothing to put in the missing one.
`ExprKind::Range` with a `None` end reports `unsupported` rather than inventing a sentinel —
which is the right call, because the two plausible sentinels are both wrong: `T::MAX` makes
`for i in 3..` a loop that terminates (it should not) and makes `xs[3..]` depend on the
element type, and a nullable end makes every `Range` pay a branch per step for a case that
is decided at compile time.

**Why it matters.** Less than most M12 rows: the two places an open-ended range is actually
useful — counting up forever and slicing to the end — both work, because both know the
missing end from context. What does not work is passing one as an argument, which is what you
want for a function taking a range to slice with (`fn view(&self, r: Range) -> []T`) — today
such a function needs two parameters or an optional.

**What it would take.** Either three shim types (`Range`, `RangeFrom`, `RangeTo`) with
`for` and indexing accepting any of them, which is Rust's answer and means three types where
the source has one piece of syntax; or one `Range<T>` with an `end: T?` and the branch paid
only by code that holds a range as a value, the `for` head keeping its current direct
lowering. The second keeps the type count down and the common path free, and is probably the
one to build — but it is a decision about what `std` exposes, so it wants the user's view
first.
