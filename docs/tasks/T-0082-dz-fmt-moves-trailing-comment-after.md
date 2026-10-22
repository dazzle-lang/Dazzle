---
id: T-0082
title: dz fmt moves a trailing comment after { or } onto its own line
milestone: none
status: open
kind: bug
area: tooling
---

A trailing comment on a line that opens or closes a block does not stay where it was
written:

```dz
impl Copy for Image {                // opt-in: without it, Image is move-only
fn main() {
}                                    // c, b and a are dropped, in that order
```

becomes a comment on the next line inside the block, and one on its own line after the `}`.
That is a layout change, so it is allowed, but it moves a comment away from what it
annotates. Separately, the column of consecutive trailing comments is not kept, which
the README's samples rely on. The trailing-comment handling is `emit_trailing_comment` in
`crates/dz_fmt/src/lib.rs`, called after items and statements but not after a block's
opening brace. Found while formatting `examples/` on 2026-10-22.
