---
id: T-0236
title: dz fmt dropped a union payload field's default
milestone: M15
status: done
kind: bug
area: tooling
---

Found 2026-11-16 writing `tests/run/value_shapes` for T-0231: `Pair { …, weight: i32 = 1 }`
in a union came back from `dz fmt` as `weight: i32`, so a literal that left `weight` out
became `DZ0512`. The parser keeps a payload field's default and the checker uses it; the
formatter printed a variant's fields with their names, types and tags but not their
defaults, against invariant 10 (the formatter only changes layout). No corpus had a payload
default, so `formatting_preserves_the_tree` never met one.

**Fixed the same day.** A payload field is printed as a struct field is, `= default`
included. `tests/fmt/items.dz` has a variant with two defaults; without the fix,
`formatting_preserves_the_tree` fails on it.

| Piece | Where |
|---|---|
| A variant's fields, printed | `ItemKind::Union` in `Printer::item`, `crates/dz_fmt/src/lib.rs` |
| The corpus line | `union Event`'s `Heal` in `tests/fmt/items.dz` |
