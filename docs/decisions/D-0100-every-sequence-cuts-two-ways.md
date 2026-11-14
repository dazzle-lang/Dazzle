---
id: D-0100
title: Every sequence cuts two ways, and text says its unit, byte or character
status: decided
---

**Arrays, slices and `List` all cut with `xs[a..b]`, `xs[a..]` and `xs[..b]`, which panic
when the range does not fit, and have `xs.slice(a, b)`, which gives `null` there instead,
and `xs.get(i)` beside `xs[i]`. A type of the program's own cuts with `[a..b]` by
implementing `Slice`, as it indexes by implementing `Index`. Text has no brackets: every way
into it names its unit, byte or character, and `len()` counts characters, `byte_len` bytes.
Chosen by the user on 2026-11-14 (T-0098); amends D-0050 and D-0069.**

D-0050 gave `xs[a..b]` to arrays, slices and pointers. A `&str` could not be cut except
through `String::slice` or `unsafe`, so a tokenizer taking its program as a `&str` had to
take a `&String` instead (`tests/bench_programs/expr`, T-0090), and a `List` had to be cut
through `xs.as_slice()[a..b]`. The user asked that it be uniform.

**Why both forms.** They differ in what a range that does not fit means. `xs[a..b]` is for
a range the program computed and knows is valid, so a bad one is a bug and panics at the
line that cut, as `xs[i]` does. `xs.slice(a, b)` is for a range that comes from input,
where failure is a case to handle: `let part = xs.slice(a, b) or return`. Rust has the same
pair, `&s[a..b]` and `s.get(a..b)`.

**Why text is different.** First built with `text[a..b]` in bytes, as Rust has it. The user
found that a bracket on text does not say whether it counts bytes or characters, and asked
for UTF-8 to be supported natively and without ambiguity: every way in says its unit.

| | panics | `none` / `null` instead |
|---|---|---|
| one byte, `u8` | `s.byte(i)` | `s.get_byte(i)` |
| bytes `a..b`, as text | `s.cut_bytes(a, b)` | `s.slice_bytes(a, b)` |
| character `n`, `char` | `s.char(n)` | `s.get_char(n)` |
| characters `a..b`, as text | `s.cut_chars(a, b)` | `s.slice_chars(a, b)` |

and the length: **`s.len()` counts characters, `s.byte_len` bytes**, the user's choice. A
`&str`'s `s.len` field, which was bytes, is refused (`DZ0586`) rather than changing meaning
under a program; `dz fix` respells it `byte_len`. `String`'s `len()` changes meaning, from
bytes to characters, and every use in the repository that meant bytes became `byte_len()`.

What follows from it:

- **Positions handed out are bytes.** `find`, `split` and the rest give byte positions, which
  `cut_bytes` takes in a step. A cut by bytes also panics, or gives `null`, when an end falls
  inside a character, so a piece is always text.
- **Characters are reached by reading from the start**, so `len()`, `char(n)` and the `_chars`
  cuts take time in proportion to the position. D-0069 rejected indexing by character as "a
  linear walk disguised as `s[i]`"; these are not disguised, since each is a method that says
  it counts characters, and that is the amendment to D-0069.
- **`Slice` and `SliceMut`** (§11.2) are what `[a..b]` calls on a type that is not an array, a
  slice or a pointer: `cut(from, to: usize?, loc = #location)`, `to` being `none` for
  `xs[a..]`, so the sequence is evaluated once, and `loc` the line that cut. `SliceMut`'s
  `cut_mut` is chosen where `xs` may be written, as `IndexMut` is over `Index`. A range has
  no type of its own a program can write, so they could not be `Index` with a range.
- **`get`, `get_mut`, `slice` and `slice_mut`** on a slice come from `std`'s `Elements` and
  `ElementsMut`; an array has its slice's, and a writable slice a read-only one's as well.
  `List` has its own.

Spec: §5.3, §11.2, §14.4.
