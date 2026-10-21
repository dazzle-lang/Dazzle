---
id: T-0021
title: An untyped imported integer is not range-checked
milestone: M7b
status: open
kind: deferral
area: checker
where: `dz_check/src/check.rs`, imported constant in a typed context
---

**What goes wrong.** A Dazzle integer literal is checked against the type its context wants:
`let b: u8 = 300` is an error at the literal, naming the range. An **imported** C constant is
untyped in the same way — it takes its type from context — but gets no such check:

```dz
let b: u8 = rl::SOME_BIG            // reaches C++ as a narrowing static_cast
```

The value is truncated, silently, in the generated C++.

**Why it matters.** It is a hole in the one rule §5 is strictest about: **no numeric
conversion is implicit**. Everything written in Dazzle obeys it; a value that came from a
header does not, so the single place where a constant's magnitude is least obvious to the
reader — a header they did not write, often a hex flag — is the place where the check is
missing. A truncated flag constant does not fail, it just does the wrong thing.

**Why it was accepted.** Imported constants in narrow contexts had not come up; the importer
records the value, so nothing was lost, only unchecked.

**What it would take.** Small, and the pieces are already on both sides: **the importer knows
the value** (it reads the enumerator or macro's integer out of libclang), and **the checker
already has the comparison** — it is what it does to a literal. So the work is to carry the
constant's value onto the symbol in a form the checker can read, and then route an imported
constant in a typed context through the same range check a literal takes, reporting at the
use with the same code and message shape.

Two details worth deciding while doing it: a constant whose value libclang could not evaluate
(a macro that is an expression over other macros) has no value to check, and should be
allowed through rather than refused — but that should be recorded, not invisible. And an
imported constant used where a *wider* type is wanted needs no check and no cast, which is
worth asserting in a test so the fix does not add a `static_cast` where none is needed.

A `typeerrors/` case and a matching `typed/` case (invariant 8): the first narrowing a big
constant, the second widening it.
