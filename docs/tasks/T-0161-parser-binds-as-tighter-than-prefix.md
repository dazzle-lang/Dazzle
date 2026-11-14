---
id: T-0161
title: The parser binds as tighter than prefix -, against §11.1's table
milestone: none
status: done
kind: bug
area: language
---

**Done 2026-11-14, r124**, the user's choice of option A. `as` is parsed by
`parse_casts`, after a prefix expression and its postfix operators and before the infix
loop, so `-x as u8` is `(-x) as u8`. Of every `.dz` file in the repository only
`tests/run/math` parses differently (`-2.5 as f32`, `-7 as i32`), to the same values. The
formatter already followed §11.1. Test: `prefix_operators_bind_tighter_than_as`.

Found building T-0129 (2026-11-03).

**Where the code is**

| Piece | Where |
|---|---|
| `as`, parsed in the postfix loop beside `()`, `[]` and `^` | `dz_parse/src/expr.rs`, the `TokenKind::As` arm (~321) |
| Prefix `-`, `!`, `~`, `&` | `dz_parse/src/expr.rs`, `parse_unary_operand` (~183) |
| The rule | §11.1: prefix operators at level 2, `as` at level 3 |

**What it is.** §11.1 puts the prefix operators above `as`, so `-x as u8` should be
`(-x) as u8`. The parser handles `as` in its postfix loop, so it binds tighter than any
prefix operator and the expression is `-(x as u8)`:

```dz
fn f(x: f64) -> u8 {
    return -x as u8      // dz parse: unary - (cast u8 (path x))
}
```

With `x = 5.5`, §11.1's reading gives `(-5.5) as u8`, which saturates to 0 (r109). The
parser's reading gives `-(5 as u8)`, which wraps to 251. `!a as i32` and `&x as ^u8` are
read the same way.

**Why it matters.** The two readings give different values, silently, and the spec is what a
reader goes by. `-1.5 as u8` is the natural way to write the example in §10.6, which had to
be parenthesised.

**Why it is this way.** Parsing `as` as a postfix is the simplest place to put an operator
that takes a type on its right, and nothing tested `as` under a prefix operator.

**What it would take.** A choice, and it is the user's, since either way changes what a
program means:

- **A. Follow §11.1**: move `as` out of the postfix loop to a binary level just below the
  prefix operators, so `-x as u8` is `(-x) as u8`, as in Rust. Add a parser test, and look
  for `-… as` in `std` and the examples, whose meaning would change.
- **B. Follow the parser**: move `as` up to level 1 in §11.1, a §21 row.
