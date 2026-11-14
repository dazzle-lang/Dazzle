---
id: D-0101
title: "{} writes a float in the shortest digits that read back, in plain decimal"
status: decided
---

**`{}` writes an `f32` or `f64` as the shortest decimal digits that read back to the same
value, laid out in plain decimal with no exponent, and with no point when the value is
whole: as Rust's `{}` writes it. An `f32` is written as an `f32`. Infinities and NaN are
`inf`, `-inf` and `NaN`, and negative zero `-0`. `{:.N}` keeps its `N` digits after the
point. Chosen by the user on 2026-11-14 (T-0223); spec §12.9.**

| Value | `{}` writes | `%g`, which it replaces, wrote |
|---|---|---|
| `1.0` | `1` | `1` |
| `0.1 + 0.2` | `0.30000000000000004` | `0.3` |
| `123.456789` | `123.456789` | `123.457` |
| `1000000.0` | `1000000` | `1e+06` |
| `0.0000001` | `0.0000001` | `1e-07` |
| `1e21` | `1000000000000000000000` | `1e+21` |
| `2.0_f32.sqrt()` | `1.4142135` | `1.41421` |
| `0.1_f32` | `0.1` | `0.1` |
| `0.0 / 0.0` | `NaN` | `nan` or `-nan` |

**Why.** `%g` kept six significant digits, so a value printed and read back was not the value
it had been: a JSON or configuration writer built on `std` could not write a number as it
read it, and an `f32` lost its seventh digit. It was also slow: `snprintf` was most of the
`json` benchmark's write (T-0173, T-0225).

**The options weighed.** The shortest round-trip digits were not in question; the layout
was. Only beyond some magnitude in exponent form, as JavaScript writes (`1e21`, `1e-7`), so
that huge and tiny values stay short; or whichever of fixed and exponent is shorter for each
value, as C++'s `std::to_chars` does by default (`1e+06` for a million); or never, as Rust's
`{}` does. The user chose **never, as Rust**: nothing a program prints surprises with an
`e`, and `f64::MAX` is 309 digits. Whole floats print `1`, not `1.0`, as before and as Rust
and JavaScript print them.

**How it is built.** The runtime's `format_float` finds the shortest digits with
`std::to_chars`'s scientific form where the C++ library has it for floats, and otherwise with
`%.*e` at one more digit each time until the text reads back, which gives the same digits;
`float_text_is_the_same_without_to_chars` holds the two to the same text. *Fixed*
`to_chars` is not used: for a large whole number it writes the exact digits rather than the
shortest padded with zeros (`f64::MAX`'s 309 digits, where Rust writes
`17976931348623157` and zeros), as the C++ standard breaks that tie.
