---
id: T-0098
title: A str cannot be cut: str[a..b] is refused, and only String has slice
milestone: M12
status: done
kind: feature
area: stdlib
where: `crates/dz_project/src/std/core/ops.dz` (`Slice`, `SliceMut`), `core/elements.dz`, `collections/list.dz`, `text/text.dz`; `crates/dz_check/src/check.rs` (`slice_range`, `slice_call`)
---

**Done** (2026-11-14, r129, D-0100: the user's choice, amending D-0050 and D-0069).
Arrays, slices and `List` cut with `xs[a..b]`, which panics, and have `xs.slice(a, b)` and
`xs.get(i)`, which give `null` there. A type of the program's own cuts with `[a..b]` by
implementing `Slice`, as `[i]` is `Index`. Text has no brackets: every way in names its
unit, `byte(i)`, `cut_bytes(a, b)`, `char(n)`, `cut_chars(a, b)` and their `get_` and
`slice_` twins, and `len()` counts characters, `byte_len` bytes (`s.len` is `DZ0586`).
`tests/run/cutting` and `tests/run/cut_panics` run all of it; `typed/` and `typeerrors/`
have the cases.

## Where the code is

| What | Where |
|---|---|
| `Slice` and `SliceMut`, what `[a..b]` calls | `crates/dz_project/src/std/core/ops.dz` |
| `get`, `slice` and their `_mut` on a slice, `Elements` | `crates/dz_project/src/std/core/elements.dz` |
| `List`'s `get`, `slice`, and its `Slice` and `SliceMut` | `crates/dz_project/src/std/collections/list.dz` |
| text by unit, `len()` in characters (the `Text` trait) | `crates/dz_project/src/std/text/text.dz`; `String::byte_len` in `string.dz` |
| `s.byte_len`, and `s.len` refused (`DZ0586`) | `crates/dz_check/src/check.rs`, `field_type`, the `Ty::Str` branch |
| text in brackets refused, naming both units | `crates/dz_check/src/check.rs`, `is_text`, `text_has_no_brackets` |
| `[a..b]` routed to `Slice`, built in for arrays, slices and pointers | `crates/dz_check/src/check.rs`, `slice_range`, `slice_call` |
| an array, and a writable slice, finding a slice's methods | `crates/dz_check/src/check.rs`, `call`, the last branches for receivers that are not structs |
| a writable slice made for a `&mut self` call | `crates/dz_cpp/src/lib.rs`, `view_arg`; `runtime.rs`, `view_arg` |
| an optional read-only slice from a writable one | `crates/dz_cpp/src/runtime.rs`, `Opt(const Slice<U>&)` |

The text below is the task as it was written.

**What it is.** A tokenizer holding the program as a `str` cannot take a token out of it
safely:

```dz
fn tokenize(text: str) { let word = text[start..i] }      // DZ0515: `str` cannot be sliced
```

The ways to cut one today are `String::slice(from, to) -> str?`, which needs the owning
`String` rather than a view, and `unsafe { (text as []u8)[a..b] as str }`. The standard
library itself uses the unsafe one, in private helpers in `text.dz` and `path.dz`.
`tests/bench_programs/expr` (T-0090) had to take `&String` instead of `str` for this reason
alone.

**Why it matters.** Splitting text into pieces is what parsers, config readers and
command-line handling do. Making the safe way unavailable on the type those functions take
pushes them towards `unsafe`.

**Why it is this way.** D-0050 gave `xs[a..b]` to arrays, slices and pointers. A `str` is
not a slice, and cutting one has to land on character boundaries, which a byte range does
not promise.

**What it would take.** Library work only: `slice(from, to) -> str?` on the `Text` trait,
with the boundary check `String::slice` already does, so `str` and `String` both have it
and `String::slice` becomes the trait's. Making `s[a..b]` itself work on a `str`, panicking
off a boundary, would be a language change for the user to decide (D-0050, a §21 row).
