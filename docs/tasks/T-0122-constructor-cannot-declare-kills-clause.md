---
id: T-0122
title: A constructor cannot declare a kills clause
milestone: none
status: open
kind: bug
area: ownership
---

Found 2026-11-01 while testing T-0117's constructors. A constructor that changes what a
`&mut` parameter reaches cannot be written at all:

```dz
struct Registry {
    pub id: usize

    Registry(all: &mut List<i32>) {
        all.push(1)            // DZ0415: `constructor` kills `all@items`, and its
        id = all.len()         // `kills` clause does not say so
    }
}
```

and `Registry(all: &mut List<i32>) kills all@items { … }` is a parse error (`DZ0202`,
"expected a member, found keyword `kills`"). The check asks for a clause the grammar does
not allow, so the only way out is a free function that builds the value.

**This is the user's call** (a grammar change, so a §21 row): either a constructor takes a
`kills` clause after its parameters as a function does (spec §6.7, §20), or a constructor
may not kill and `DZ0415` there says so and points at a factory function instead.

**Where the code is.**

| Piece | Where |
|---|---|
| A constructor member's parse, which reads no clause: `parse_ctor` | `dz_parse/src/item.rs` |
| `DZ0415` raised for it, with the clause it would need | `dz_flow/src/ties/report.rs`; the constructor's signature in `body_signature`, `dz_flow/src/ties/body.rs` |
