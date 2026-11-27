---
id: T-0274
title: "An aliasing matrix: every pair of arguments against every rule that keeps them apart"
milestone: M19
status: done
kind: feature
area: testing
---

**Done 2026-11-27**, asked by the user after three holes in M19 were each found only by
asking "what if I pass the same list twice?". Each rule had a test; nothing tried every pair
against every rule.

`crates/dz_driver/tests/aliasing.rs` writes one program per **use** and **pair** and analyses
it. A pair is two arguments and whether they may be one place: one local twice, two locals, a
field twice, two fields, an element twice, two constant elements, `ls[i]` beside `ls[j]`, the
value inside an `Rc` beside that `Rc` or another to it, and a local beside an `Rc`. A use is
what receives them: a function that kills through the first, one that kills and keeps a view
through the second, one that only writes, and a `parallel` block or `parallel for` writing
through the first while reading the second, in place or inside a function. A pair that may
alias must be refused with the use's codes; a pair that cannot must be accepted.

What it found the first time it ran, each listed in its `KNOWN` with its task, so the test
fails the day one is fixed until it is taken off: T-0268 (a function's parameters, by a block or
a loop inside it, fixed the same day by r158), T-0272 (a kill beside an `Rc` to the same value, on one thread: a read of
freed memory, fixed the same day) and T-0273 (a `parallel for` over a list's element, fixed the same day).

**Extending it.** A new rule about two places adds a `Use`; a new way to reach a place adds a
`Pair`. Both are one row.

## Where the code is

| Piece | Location |
|---|---|
| The matrix | `crates/dz_driver/tests/aliasing.rs`: `PAIRS`, `USES`, `KNOWN` |
| Running cases on every core | `crates/dz_driver/tests/parallel/mod.rs` |
