---
id: T-0005
title: Format specifiers
milestone: M12
status: done
done: r66, 2026-10-21
kind: feature
area: text
---

**What a format specifier is.** In a formatting macro (spec §12.9) a placeholder is `{}`,
and anything after a colon inside it is the *specifier*: it says not *which* value to write
but *how* to write it. Dazzle has two today:

```dz
println!("{:x}", flags)      // hex:                255     -> "ff"
println!("{:X}", flags)      // hex, upper case:    255     -> "FF"
println!("{:.2}", elapsed)   // 2 digits after the point: 3.14159 -> "3.14"
```

**What is missing** is the other half of the set every language with formatting has: a
**width**, and **zero padding**.

```dz
println!("{:8}", score)      // right-aligned in 8 columns: "    1250"
println!("{:08x}", addr)     // 8 columns, zeros rather than spaces: "0000beef"
println!("{:<8}|", name)     // left-aligned, if alignment is wanted too
```

**Why it matters.** Width is what makes several lines line up in a column, and that is most
of what on-screen text in a game is: a HUD with a score and a timer that must not jitter as
the digits change, a debug overlay of entity rows, a table of frame timings. Without it the
only way to line anything up is to count characters by hand and pad with `str` concatenation
at the call site. Zero padding is what makes an address, a colour or a hash readable —
`{:08x}` rather than `ff`, `beef` and `0000beef` all looking like different kinds of thing.

**What it would take.** The machinery is mostly in place, and in the right shape:

- The specifier is parsed and checked at compile time in `dz_check/src/check.rs` —
  `spec_fits` says which specs suit which types (`x`/`X` an integer, `.N` a float) and
  `check_spec` reports a mismatch at the placeholder. Two more arms, plus the rule for
  which types a width suits (all of them) and which a zero fill suits (numbers).
- The specifier reaches the runtime as the string after the colon, and
  `Formatter::set_spec` in `std/core/formatter.dz` parses it into `m_hex`, `m_upper` and
  `m_precision`, read back by `hex()`, `upper()` and `precision()`. A width wants two more
  of those — `width()` and `fill()` — so a type with its own `Format` can honour them.
- **The one real design question**: padding has to wrap the *whole* of what an argument
  writes, and an argument writes itself by calling `write_str` on the formatter directly.
  So either `write_args` runs each argument with a width into a scratch buffer and pads the
  result before emitting it (a capture mode on `Formatter`, which already has `m_buf`), or
  the built-in `Format` impls pad themselves and a type of one's own is simply handed the
  width to use as it likes. The second is cheaper and matches how `hex()` already works;
  the first is the only one that pads a user type without its cooperation.

**It is a language change**: §12.9's spec table is part of the contract, so this gets a new
row in the table, a §21 revision row, and a case in `tests/run/formatting` plus a
`tests/errors` file for each new way to spell the spec wrongly. The spelling above is the
one the user named when the task was written; `{:<8}` / `{:>8}` alignment is a further
question, not settled.

Related: [T-0055](T-0055-pointer-has-no-format.md) (a pointer has no `Format`),
[T-0056](T-0056-formatting-macro-arguments-go-through-format.md) (arguments go through
`&Format`), [T-0057](T-0057-spread-cannot-be-formatted.md) (a spread cannot be formatted).
