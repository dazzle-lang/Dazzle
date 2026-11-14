---
id: T-0216
title: A for range's end is evaluated again before every pass
milestone: M15
status: done
kind: bug
area: codegen
where: `crates/dz_cpp/src/lib.rs`: `for_loop`, the counting loop
---

**Done** (2026-11-14), with D-0099 and r128. Found by T-0215.

| Piece | Where |
|---|---|
| The counting loop, `for i in a..b` | `Emitter::for_loop` in `crates/dz_cpp/src/lib.rs` |
| The rule | spec §10.4, D-0099 |
| Test | `tests/run/range_once` |

**What was wrong.** `for i in a..b` was emitted as `for (T i = a; i < b; ++i)`, so `b` was
evaluated again before every pass, as a C++ counting loop checks its condition. The spec did
not say which; the user chose once (D-0099), as Rust's `for` and C++'s range-`for` do.
`for i in 0..xs.len() { xs.push(…) }` never ended, and `for i in 0..next()` called `next()`
on every pass.

**What it does.** The end is held in a variable declared with the counter, `for (size_t i =
0, _dz_end1 = xs.len(); i < _dz_end1; ++i)`, so the two are evaluated once, start first. An
end that is a literal or a constant is still written in place, since reading it again
changes nothing and the loop then reads as it was written.

**Left.** `for i in 0..=hi` is emitted as `i <= hi`, which never ends when `hi` is the
largest value of its type, since the counter wraps (§5.1).
