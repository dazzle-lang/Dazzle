---
id: T-0097
title: Moving a union through a fallible result costs a construct, two moves and a drop at every step
milestone: M15
status: open
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
