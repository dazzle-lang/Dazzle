---
id: T-0276
title: "Extend the aliasing matrix: slices, view structs, tuples, receivers, generic callees, nested blocks"
milestone: M19
status: open
kind: feature
area: testing
---

The aliasing matrix (T-0274, `crates/dz_driver/tests/aliasing.rs`) found three holes the first
time it ran, so what it does not try is where the next ones are likely to be. It tries ten
pairs, all `List<i32>` places, against eight uses. Not yet:

- **Pairs:** a slice of a list beside the list or another slice of it (`xs[0..2]`, `xs[1..]`);
  a view struct holding a reference beside what it views; a tuple's two parts; a `Box`'s
  value beside the box; a `Weak` beside an `Rc`; an element of an array beside the array; a
  value reached through `&mut self` beside a field of `self`.
- **Uses:** a method whose receiver is the first argument (`&mut self` and `&self`); a generic
  callee; a function value; a block nested in a call of another block; a `parallel for`
  inside a block's call; a function that passes its parameters on to one that `lends`.

Each row is one line (`Pair` or `Use`). A failure goes in `KNOWN` with a task of its own, as
T-0268, T-0272 and T-0273 did. T-0275's cases, where the analysis gives up, belong here too.

## Where the code is

| Piece | Location |
|---|---|
| The matrix | `crates/dz_driver/tests/aliasing.rs`: `PAIRS`, `USES`, `KNOWN`, `SETUP` |
