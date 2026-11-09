---
id: T-0178
title: A for loop over a view of a temporary reads freed memory: for c in format!(…).chars()
milestone: none
status: open
kind: bug
area: codegen
---

Found on 2026-11-09 while writing T-0171's frame capture. Safe code, no `unsafe`
anywhere, reads freed memory: a hole in §1.1's promise.

**What it is.** A `for` whose head calls a method returning a view of a temporary iterates
over freed memory:

```dz
let mut out = List<u8>()
for c in format!("P6\n{} {}\n255\n", 1280, 720).chars() {   // wrong: reads freed text
    out.push(c as u8)
}
// out is 60 253 253 253 42 …, not 80 54 10 49 … ("P6\n1…")

let header = format!("P6\n{} {}\n255\n", 1280, 720)         // right: the String outlives
for c in header.chars() { out.push(c as u8) }                //   the loop
```

It reproduces with the release `dz` built on 2026-11-04 (0.1) and with `DZ_REGIONS=0`, so
it is neither placement nor recent.

**Why it happens.** The loop is emitted as a C-style `for` whose init statement holds the
whole head:

```cpp
for (auto _dz_it1 = ::dz::obj([&](…) { /* format! */ }(…)).chars(); valid(_dz_it1); advance(_dz_it1))
```

C++ destroys the `String` the lambda returned at the end of that init statement, before
the first iteration, so `_dz_it1` points into freed text. It is the same trap as C++'s own
range-for over `f().chars()`, which C++23 fixed (P2718). Nothing on the Dazzle side refuses
it: the iterator is tied to the temporary, and the temporary is taken to live as long as
the loop that uses it.

**Why it matters.** It is a use-after-free in safe code, the class of bug D-0094 exists to
rule out, in a form people write all the time (`for line in read_text(p).lines()`,
`for w in s.to_ascii_lowercase().split_whitespace()`).

**What it would take.** Compiler work, unless the user prefers the refusal:

1. **Keep the temporary alive for the loop** (recommended, as Rust and C++23 do): when the
   head's receiver is a temporary, emit it into a local before the loop, `auto _dz_seq =
   <temporary>; for (auto it = _dz_seq.chars(); …)`, so it is dropped after the loop. The
   `for` emission is in `dz_cpp/src/lib.rs`; `_dz_seq2` already exists for the range-for
   form, so the C-style form needs the same hoisting.
2. **Refuse it** in the ties check: a view of a temporary used past its statement. That is
   a language change (a §21 row) and makes a common, harmless-looking loop an error.

Either way, add a `tests/run` program that iterates a view of a `format!`, of a function's
returned `String`, and of a `List` returned by value, checked under the sanitizer job.
Also check `while` heads, `if` heads and `switch` subjects for the same shape.
