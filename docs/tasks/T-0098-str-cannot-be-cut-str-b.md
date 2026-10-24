---
id: T-0098
title: A str cannot be cut: str[a..b] is refused, and only String has slice
milestone: M12
status: open
kind: feature
area: stdlib
where: `crates/dz_project/src/std/text/text.dz` (the `Text` trait), `string.dz` (`String::slice`)
---

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
