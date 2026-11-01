---
id: T-0129
title: Division by zero, `MIN / -1`, oversized shifts and out-of-range casts are undefined in C++
milestone: none
status: open
kind: design
area: codegen
---

Point 5 of [D-0094](../decisions/D-0094-safe-code-has-no-undefined-behaviour.md), which is
open. It extends D-0063.

**Where the code is**

| Piece | Where |
|---|---|
| Binary operators, emitted as `(a op b)` | `dz_cpp/src/lib.rs`, `binary` (~4549) |
| Compound assignment (`/=`, `<<=`, …) | `dz_cpp/src/lib.rs`, around `AssignOp` (~3580) |
| `as` between numbers: a `static_cast` | `cast` in `dz_cpp/src/runtime.rs` (~623) |
| The flags (`-fwrapv`) | `compile` in `crates/dz/src/build.rs` |
| The rules | §5.1, §10.6, §11; D-0063 |

**What it is.** `-fwrapv` makes `+`, `-` and `*` wrap, as D-0063 says. It covers none of
the following, which the spec does not mention and C++ leaves undefined:

```dz
let min: i32 = -2147483648
println!("{}", min / minus_one())   // the process dies on a CPU trap (0xC0000095 on Windows)
println!("{}", 10 / zero())         // the same, or anything
println!("{}", 1 << forty())        // a shift past the width
println!("{}", huge() as i32)       // 1e20 does not fit in an `i32`
```

**Why it matters.** These are ordinary operations in ordinary code. A division by a count
that can be zero is a common bug, and today it is undefined behaviour rather than a panic
naming the line.

**Why it is this way.** Codegen maps each operator straight onto C++'s, and the spec never
said what these cases do.

**What it would take.** Proposed: D-0063's "wraps, never panics" where an answer exists,
and the bounds check's panic where none does.

| Operation | Proposed | C++ today |
|---|---|---|
| integer `a / 0`, `a % 0` | panic, with the location | undefined |
| `MIN / -1` | `MIN` (wraps) | undefined (x86 traps) |
| `MIN % -1` | 0 | undefined |
| `a << n` or `a >> n`, with `n` past the width or negative | `n` taken modulo the width, as `wrapping_shl` | undefined |
| a negative value shifted left | wraps | undefined before C++20 |
| float `as` integer, out of range | saturates at the type's `MIN` or `MAX` | undefined |
| NaN `as` integer | 0 | undefined |

These are Rust's rules, except that Rust panics on `MIN / -1`, where Dazzle wraps as D-0063
does for overflow. The cost is a compare and branch per division by a non-constant, a mask
per shift by a non-constant, and a clamp per float conversion; constants fold away. The
helpers go in the shim (`::dz::div`, `::dz::rem`, `::dz::shl`, `::dz::shr`, a checked float
conversion), warning-clean on clang++ and g++ (invariant 7). Any constant folding in the
checker must agree, and a constant `/ 0` can be a compile error.

A language change: §5.1, §10.6 and §11, with a §21 row.
