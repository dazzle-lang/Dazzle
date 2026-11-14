---
id: T-0223
title: "{} prints a float with C's %g: six significant digits, not round-trip, and slow"
milestone: M12
status: done
kind: design
area: stdlib
where: `crates/dz_project/src/std/core/format.dz`: `write_float`, `format_float` (an `extern fn`); `crates/dz_cpp/src/runtime.rs`: `format_float`
---

Found by the `json` benchmark (T-0173, 2026-11-14). **The user's decision**: it changes what
every program that prints a float prints.

**What it does now.** `{}` on an `f32` or `f64` goes through `write_float`, which calls C's
`snprintf` with `%g` (and `%.*f` for `{:.N}`). `%g` keeps **six significant digits**, so:

- it is not round-trip: `123.456789` prints `123.457`, and an `f32` loses its seventh digit.
  A JSON or config writer built on `std` cannot write a number back as it read it; the
  `json` benchmark generates numbers of at most six digits so that its two halves agree;
- `1234567.0` prints `1.23457e+06`, switching to an exponent at a million;
- it is slow: in `json`'s writer, glibc's `printf_fp` and its helpers were about 30 ms of
  the 68 the write took, where the C++ half's `std::to_chars` hardly shows.

Spec §12.9 defines `{:.N}` for floats and says nothing of `{}`, so this is unspecified
rather than decided.

**The usual answer** is the shortest text that reads back to the same value, as Rust,
Python, JavaScript, Go and C++17's `std::to_chars` print: `0.1` is `0.1`, `123.456789` is
`123.456789`, and an `f32` is printed as an `f32` (`0.1f32` is `0.1`, not the `f64`
`0.10000000149011612` its widening gives, which is a second thing to fix: `Format for f32`
widens to `f64` first). The cost is that some values print longer than they do now: `0.1 +
0.2` is `0.30000000000000004`, where `%g` shows `0.3`, and `tests/run`'s expected output
changes wherever a float is printed.

**What it would take**, once decided: `format_float` in the runtime calls `std::to_chars`
(shortest, or with a precision for `{:.N}`), available for floats in libstdc++ 11, libc++
14 and MSVC 2019, with `snprintf("%.17g")` and trimming where it is not; `Format for f32`
passes the `f32`; spec §12.9 states what `{}` prints; a revision row in §21; and every
expected output reviewed. The `json` benchmark could then use numbers of any length.

**Done 2026-11-14 (r130, D-0101).** The user chose Rust's `{}`: the shortest digits that read
back, always in plain decimal, `1` for a whole float. `format_float` in the runtime finds
the digits with `std::to_chars`'s scientific form, or `%.*e` where the library lacks it
(the two agreed on 200,000 random `f64` and `f32` bit patterns, and every `f64` matched
Python's `repr`), and lays them out itself; `Format for f32` passes the `f32`; an
`error(...)` message formats floats the same way. `tests/run/float_text` pins the text, and
`float_text_is_the_same_without_to_chars` the fallback. `formatting` and `math` print
more digits where six were not enough.
