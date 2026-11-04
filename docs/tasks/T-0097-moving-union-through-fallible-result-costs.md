---
id: T-0097
title: Moving a union through a fallible result costs a construct, two moves and a drop at every step
milestone: M15
status: done
kind: feature
area: codegen
where: `crates/dz_cpp/src/lib.rs`, the lowering of `or` on a `!->` call and of `let`; `std/memory/box.dz`
---

**What it is.** A recursive-descent parser written the obvious way returns its tree through
`!->` at every level:

```dz
fn term(&mut self) !-> Expr {
    let mut lhs = self.unary() or return
    ...
    return move(lhs)
}
```

For each `let x = f() or return`, the generated C++ default-constructs an `Expr`,
move-assigns the result's value into it, then move-constructs `lhs` from that. The `return`
moves it once more into a new `Res<Expr>`. Each move of a union switches on its tag, moves
the payload, and resets the source to its default (spec §6.4). Each reset temporary is then
dropped, and the drop checks the tag again. C++ returning `Result<std::unique_ptr<Expr>>`
moves one pointer.

`Box<T>` adds to it. A `Box` is 40 bytes (pointer, allocator, size, alignment, drop
function) against `unique_ptr`'s 8, so a `Binary{ op, lhs: Box, rhs: Box }` node is about
88 bytes and every move copies all of them. Every `Box(...)` also allocates through the
`Allocator` interface rather than calling `operator new` directly.

In `tests/bench_programs/expr` (T-0090), parsing 100,000 statements takes about 120 ms in
Dazzle. The C++ half parses *and* evaluates them in about 80 ms. Tokenizing (about 1.4×)
and evaluating (close to even) are not where the gap is.

**Why it matters.** Trees are everywhere in tools and games: syntax trees, scene graphs,
behaviour trees. The cost is invisible in the source, and it is exactly the "reset to
defaults on every move" cost a reader would suspect Dazzle of. Here it is real.

**What it would take.** Compiler work, plus a library question.

1. Lower `let x = f() or return` on a `!->` call to construct `x` straight from the
   result's value: one move, no default-constructed temporary.
2. Where the source of a move is a temporary that is never read again (a result, or the
   operand of a `return`), skip resetting it, since nothing can observe the reset. That
   keeps §6.4 as it is for everything a program can name.
3. Ask the user whether `Box` should shrink: the size, alignment and drop function are
   needed to free through any allocator and to drop a `Box<Base>` holding a `Derived`
   (D-0064). A `Box` from the global allocator could keep only the pointer and a drop
   function. That is a library decision, not a language one.

Re-run `python tools/bench_programs.py expr` before and after.

**Where the code is**

| Piece | Where |
|---|---|
| `or` whose clause never falls through, read in place | `or_in_place` in `dz_cpp/src/lib.rs`, called from the `ExprKind::Or` arm |
| `Res`'s constructors and `good`, by reference | `struct Res` in `dz_cpp/src/runtime.rs` |
| A union's move, which leaves its source's variant; `take` for a source a program can still reach | `union_decl`, `moved_out` in `dz_cpp/src/lib.rs` |
| `Box`: pointer, allocator, and `drop_boxed_as<T>`, which drops and frees | `std/memory/box.dz` |
| A constructor's parameter it always moves out, taken by `&&` | `consumed_params` in `dz_cpp/src/lib.rs`, used by `decl_params`, `take_ownership_of_params` and `heap_ctor` |

**Done so far** (2026-11-04). Point 1, and a move the task did not list:
- `x or return`, and any `or` whose clause is `return`, `break`, `continue`, a panic or
  `unreachable`, no longer builds a default value for the clause to fill. The value is
  moved straight out of the result where it is used: one move where there were two, and no
  default to drop. A clause that yields a value still uses the temporary.
- `Res`'s constructor and `Res::good` took their value by value, one more move per
  `return`; they take it by reference now.

`become` went from 27 million calls to 14.7 million. Measured with `bench_programs.py`,
seven alternating runs, Dazzle / C++: `expr` 2.10× → 1.54× on clang++ and 1.66× → 1.26×
on g++; `astar` 1.19× → 1.07× and 1.12× → 1.04×; `words` 1.23× → 1.15× and 1.27× → 1.15×.
`tests/run/optionals` prints two fewer `drop 0`s: those were the drops of the default
temporaries, which no longer exist.

Point 2, for unions: a union's move no longer resets its source, whose payload fields are
each at their own defaults after the move. A source a program can still reach (a field, an
element, through a pointer, a static, an optional `or` or `if let` consumes) is reset by
`take` instead (`moved_out` in `dz_cpp`). 11% on clang++, 1% on g++, measured against the
same C++ with the reset put back. With all three, `expr` against C++ went from 2.10× to
1.50× on clang++ and from 1.66× to 1.33× on g++.

**Left.** Point 3 (`Box`'s size, a library decision for the user). `Box(value: T)` moves
its value twice, into the parameter and then into the allocation, and a union literal is
built as a default, switched to its variant, then filled field by field.

**Done** (2026-11-04), the rest:
- **`Box` is 24 bytes, from 40**, the user agreeing to shrink it. One function made for
  the type the box was built as now drops the value and frees it at that type's size, so
  the box keeps the pointer, its allocator and that function. A `Box<Base>` holding a
  `Derived` and any allocator work as before.
- **`Box(value)` moves its value once, from three times.** A constructor's parameter that
  owns memory, cannot be copied, and is moved out on every path before any `return` takes
  its argument by `&&` in C++.
- **A union literal built straight into its variant** measured within noise by hand-edited
  C++, so it is left as it is.

`expr` against its C++ half: 1.17× on clang++ and 1.10× on g++, from 2.10× and 1.66× when
this task was opened. `docs/log/tooling.md` has the detail.
