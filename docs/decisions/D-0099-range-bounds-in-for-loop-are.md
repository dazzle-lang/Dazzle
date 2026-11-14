---
id: D-0099
title: A range's bounds in a for loop are evaluated once, before the first pass
status: decided
---

**`for i in a..b` evaluates `a`, then `b`, once, before the first pass, as Rust does;
chosen by the user on 2026-11-14 (T-0216).**

Until then the compiler emitted `for (i = a; i < b; ++i)`, as a C++ counting loop is
written, so `b` was evaluated again before every pass. The spec did not say which. Found
while making loops over a `List` fast (T-0215): `for i in 0..xs.len() { xs.push(…) }`
never ended, and `for i in 0..next()` called `next()` on every pass.

The two options:

1. **Once, before the first pass.** What `for i in a..b` reads as: the range is a value,
   `a..b`, and the loop walks it. Rust's `for` and C++'s range-`for` both evaluate their
   range once. Chosen.
2. **Again before every pass**, as C++'s three-part `for` checks its condition. A loop
   whose bound shrinks as it works could be written that way; it is a `while` loop, and
   should be spelt as one.

What follows from it:

- The bounds are evaluated left to right, `a` then `b`, as every operand is (D-0049).
- Changing what `b` was computed from inside the loop does not change how many passes
  there are: `for i in 0..xs.len()` runs as many passes as `xs` had elements when it
  started, however many the body pushes.
- A range held in a variable, `let r = 0..n; for i in r`, was a value already, and is
  unchanged.

Spec: §10.4.
