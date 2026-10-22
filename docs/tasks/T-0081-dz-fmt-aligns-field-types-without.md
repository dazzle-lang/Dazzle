---
id: T-0081
title: dz fmt aligns field types without counting pub
milestone: none
status: open
kind: bug
area: tooling
---

A run of fields is aligned on the width of the longest *name*, but the visibility prefix is
written before the name and left out of that width, so a struct mixing `pub` and private
fields comes out ragged:

```dz
struct Image {
    pub width:    u32
    pub height:   u32
    m_pixels: own ^mut u8
}
```

`members` in `crates/dz_fmt/src/lib.rs` measures `f.name` (plus `own `), and `member`
pads `lhs` without `vis`. Measuring and padding `{vis}{lhs}` together would line the types
up. Every formatted file with such a struct changes, the standard library's included, so
the fix lands with a re-format and the `tests/fmt` snapshots. Found while formatting
`examples/` on 2026-10-22.
