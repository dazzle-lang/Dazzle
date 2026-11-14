---
id: T-0218
title: Skip the drop of a tree of Box-es in a region: a union of boxes, and a tuple let with or
milestone: none
status: done
kind: feature
area: ownership
where: `crates/dz_flow/src/release.rs`: `Whole`, `whole_functions`, `Memo::answer`, `Drops::union`, `boxed`, `reaches_inside`, `skipped_drops`; `crates/dz_cpp/src/lib.rs`: the tuple `let`
---

**Done** (2026-11-14), the user's choice after T-0217. What was done is at the end; what
was found first follows as it was written.

Found 2026-11-14 by timing `expr`'s phases against its C++ half (T-0217 is the other half
of that). A row [T-0147](T-0147-skip-more-drops-value-lent-as.md) does not have.

**What is wrong.** `expr` parses each statement into a tree of `Box<Expr>`, where `Expr` is
a union whose variants hold `Box<Expr>`s. Placement puts each statement's tree in the region
of its pass of the loop (`dz placements`: `parser.statement  region: each pass of a loop in
main`), but its drop is not skipped. So every pass walks the whole tree at its end: each
`Box`'s drop calls the stored `drop_boxed_as<Expr>` through a function pointer, which drops
the union and frees the node through the allocator's vtable, into a region that ignores the
free. 1.8 million of those calls for 100 000 statements.

**What skipping it would give.** Measured by making `Box<Expr>`'s generated destructor
return at once, which is what a skipped drop amounts to here, and compiling both versions
alike: `expr`'s parse-and-evaluate loop goes from 72.6 ms to 59 ms on clang++ 19 (its C++
half: 65) and from 69 ms to 60 ms on g++ 14 (C++: 61), output unchanged. That would put
`expr` ahead of its C++ half on both compilers.

**Why it is not skipped.** In `dz_flow/src/release.rs`:

1. **A union is never releasable**: `Drops::composite` returns false for `is_union`. A
   union's drop drops its active variant, so a union whose payloads are releasable is
   releasable too.
2. **A recursive type is never releasable**: `Memo::answer` takes a type reached through
   itself to answer `false`. `Box<Expr>` reaches `Expr`, which reaches `Box<Expr>`. For
   "only frees" and "releasable" the answer that holds is the greatest one (assume yes
   while computing, as a coinductive proof does); whether that is sound for every question
   `Memo` answers needs working through, since a wrong yes leaks rather than corrupts.
3. **The tree arrives through `let (name, tree) = parser.statement() or { … }`**: a
   `LetTuple`, whose value is an `Or`, where `skipped_drops` follows only a `Let` whose value
   is a call, a struct literal or `format!`.
4. Whether the parser's functions build their result whole in the caller's region
   (`whole_functions`, `returns_whole`) when they build it from `Box(…)`s of other results
   (`Binary{ lhs: Box(move(lhs)), … }`, which grows `lhs` from itself) has not been checked.

**Where the code is.**

| Piece | Where |
|---|---|
| Whether a type's drop only frees, and whether it is releasable | `Drops::only_frees`, `releasable`, `composite`, `holder`, `Memo` in `dz_flow/src/release.rs` |
| Which locals skip their drop | `skipped_drops`, `built_in_region` there |
| Box's drop, through `m_drop` | `std/memory/box.dz`; `Frees::call`'s `boxed` case in `release.rs` |
| The program | `tests/bench_programs/expr/src/main.dz`: `Parser`, `main`'s loop |

**What was done.**

- **Self-containing types**: `Memo::answer` takes a type being worked out to answer `true`,
  and when it answers `false` after all, forgets every answer worked out since, which may
  have leaned on it. Both questions it answers hold of a value when they hold of every part,
  and every value is finite, so the largest answer is the right one.
- **Unions** are releasable when every owning payload is and the union's own methods replace
  neither `self` nor a payload (`Drops::union`). **A `Box<T>`** is, when `T` owns nothing or
  is releasable and no struct derives from `T` (`Drops::boxed`): a `Box<Shape>` may hold a
  `Circle`.
- **One walk, `Whole`, says whether a value is built whole** where its sites put it, in a
  region or in a function's result: sites, literals (struct, union, tuple), `format!`,
  `or` (the operand, and the clause unless it diverges), and locals whose every `let` and
  every `x = …` is such a value, a local met again through its own assignment counting as
  whole by induction. A `Box(…)` is whole only when what it was given is. A local not
  moved is a copy, on the heap.
- **`whole_functions` is a greatest fixpoint**: every candidate is taken to build its
  result whole, and those that do not, given the rest, are dropped until none is. A
  parser's functions are whole together; without cycles the answer is the old one.
- **`skipped_drops` follows a tuple `let`** and an `or`, and skips a value holding a union
  or a `Box` only in a local that is not `mut`, since a `switch`'s `&mut` binding or a
  `&mut` reaching inside could put a box from elsewhere there, which `replaced` does not see
  (`Drops::reaches_inside`).
- **Codegen** builds a skipped part of a tuple `let` in storage C++ never destroys, as it
  does a `let`'s value.

`expr`'s token list and each statement's tree now skip their drops; `astar`'s and `words`'
placements are unchanged.

**Measured**, `expr`'s parse-and-evaluate loop, three runs: clang++ 72.8–73.8 ms to
66.6–69.0 (C++: 65), g++ 68.7–70.0 to 63.6–64.6 (C++: 61). The whole program, by
`bench_programs.py`: 1.10× its C++ half to 1.05× on clang++, 0.96× to 0.92× on g++.

**Left.** The experiment that made every `Box<Expr>` destructor return at once gave 59 ms,
not 67: it also skipped the destructors of the moved-from boxes and unions the parser
leaves behind (`Box<Expr>(move(lhs))`, `let value = self.expr() or return`), which no
region makes unnecessary. That is the cost of moving a union through `!->`,
[T-0097](T-0097-moving-union-through-fallible-result-costs.md)'s ground.

**Tests**: `a_tree_of_boxes_skips_its_drop_when_built_whole` in
`crates/dz_flow/tests/placement.rs`, one case per rule and one per reason to keep a drop.
