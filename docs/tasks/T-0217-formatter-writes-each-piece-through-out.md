---
id: T-0217
title: Formatter writes each piece through an out-of-line call, and decimal digits by hardware division
milestone: M12
status: done
kind: feature
area: stdlib
where: `std/core/formatter.dz`: `write_str`, `write_str_slowly`, `write_char`; `std/core/format.dz`: `write_digits`
---

**Done** (2026-11-14). Found by timing `expr`'s phases against its C++ half: generating the
text took 41 ms against C++'s 29–31, the rest of the program being close.

| Piece | Where |
|---|---|
| The fast path, into a `String` outside a width | `Formatter::write_str` (`#[inline]`), `write_char` in `std/core/formatter.dz` |
| Everything else `write_str` did | `Formatter::write_str_slowly` |
| Decimal digits | `write_digits` in `std/core/format.dz` |

**What was wrong.**

- `write_str` was one out-of-line function for every destination, so each piece `format!` or
  a `Format` implementation appended was a call: 4.8 million in `expr`'s generator. And
  `write_char`, though `#[inline]`, encoded its character into a 4-byte array and called
  `write_str` with it.
- `write_digits` divided by `base`, a parameter, so each digit cost two hardware divisions:
  the binary had `div %r9` in its loop. A division by a constant 10 is a multiply.

**What it does.** `write_str` is now `#[inline]` and appends straight to the `String` when
that is where the text goes and no width is being counted; anything else goes to
`write_str_slowly`, which is the old body. `write_char` appends an ASCII character with
`String::push`, under the same condition. `write_digits` has a loop of its own for base 10.
What is written is unchanged.

**Measured**, `expr`'s generator, 100 000 statements, three runs, `dz build --release`:

| | before | after | C++ |
|---|---:|---:|---:|
| clang++ 19 | 39.7–42.1 ms | 32.6–34.1 ms | 28.1–29.6 ms |
| g++ 14 | 42.7–43.7 ms | 32.4–33.1 ms | 30.9–31.4 ms |
