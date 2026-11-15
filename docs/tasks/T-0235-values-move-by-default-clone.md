---
id: T-0235
title: "Values move by default, `clone` replaces `copy`, and `move` is removed (D-0106)"
milestone: none
status: open
kind: feature
area: language
where: `crates/dz_check/src/lib.rs` (`copyable`, `copy_trait`, `move_only_because`); `crates/dz_flow/src/lib.rs` (`value`, `DZ0401`, `DZ0403`); `crates/dz_project/src/std/core/ownership.dz` (`Copy`); `docs/spec/06-value-semantics-ownership-copy.md`
---

**Decided by the user on 2026-11-15: D-0106.** The user's idea, from T-0234's follow-up, and
first in the backlog ahead of the performance work ("First, before the perf work"), so that
work and the new benchmarks are written once, in the new language. A language change: spec
§6 and a §21 row land with the compiler change that makes them true.

## What changes for a program

| Today | After |
|---|---|
| `let b = move(a)` | `let b = a` |
| `let b = a` of a `String`: copies the text | moves; `a.clone()` to copy |
| `let b = a` of a move-only value: `DZ0403` | moves |
| reading `a` after it moved: `DZ0401` | the same, with a fix that writes `a.clone()` at the move |
| `impl Copy for T { fn copy(&mut self, other: &Self) }` | `impl Clone for T { fn clone(&mut self, other: &Self) }` |
| `T: Copy` | `T: Clone` |
| `move(self.m_x)` | `take(self.m_x)` (proposed) |
| `let _ = move(a)` | `drop(a)` (proposed) |
| `let y = x` of an `i32`, a `Vec3`, a `&str` | unchanged: plain data copies |

How the questions were settled is in D-0106: moves by default rather than at a last use,
`move` removed rather than kept, plain data copying, `a.clone()` at the call site, `clone`
written as `copy` is, and a moved value dropped with its new owner.

## The plan

Each stage its own commit, `master` green after each, whole suite on both compilers.

1. **The compiler moves by default.** Where a by-value read of an owned place whose type is
   not plain is today a copy, or `DZ0403`, the typed tree gets a `Move` around it, as if
   `move(…)` were written. Flow, placement and codegen already treat a `Move`, so they
   need nothing new, and placement sees the moves before it places anything, which is the
   condition T-0117 set ("codegen must not turn a last use into a C++ move"). The
   classification of a value context is the one flow already makes (`value` against
   `place_only` in `dz_flow/src/lib.rs`); the insertion either moves into the checker, which
   builds the tree, or becomes a pass between checking and flow that rewrites it, and the
   pass is the smaller change. Plain is today's `copyable` minus a type with an `impl Copy`
   body of its own and anything holding one.
2. **`Clone` replaces `Copy`**: the trait in `std/core/ownership.dz`, the special-member
   checks in `dz_check` (`impls.rs`, the `prelude_trait("Copy")` sites) and `dz_cpp`, the
   parser's template for the member (`dz_parse/src/item.rs`), and `a.clone()` as a method
   every `Clone` type has, lowering to the placed copy codegen already writes
   (`copy_tag`). `DZ0401`'s fix writes `.clone()` at the move.
3. **`move` is retired**: still a keyword, so `move(x)` is a new diagnostic saying it is
   gone, with a certain fix that unwraps it; `take` and `drop` in `std`.
4. **Migrate with `dz fix`**, not by hand: `std` (53 `move(` and 4 `impl Copy`), the
   examples, the benchmarks, every corpus (`tests/run`, `tests/flow`, `tests/errors`,
   `tests/workspace`, the feature matrix), 300 `move(` in 93 files in all; then the
   snapshots, each diff read. The C++ halves of the benchmarks do not change.
5. **What a reader sees**: spec §0.1, §1 (the three special members), §6.1–§6.6, §8 and
   §14.6 where they say `copy` or `move`; a §21 row and the revision; `dz explain` for
   `DZ0401`, `DZ0403` and the new code; the VS Code grammar and snippets; the README.
6. **Measure**: every benchmark on both compilers, which should not move, since each
   already moved where it now moves by default.

Open while building, for the user if they matter: whether `take` and `drop` keep those
names, whether a generic `T` with no bound can still be copied when its instance is plain
(D-0106 says no: a generic body moves a `T` always), and whether an inlay hint shows a move
the source no longer spells.

## Where the code is

| What | Where |
|---|---|
| Whether a type copies; the `Copy` trait's symbol | `crates/dz_check/src/lib.rs`: `copyable`, `copy_trait`, `move_only_because` |
| The `Copy` trait | `crates/dz_project/src/std/core/ownership.dz` |
| A special member's checks | `crates/dz_check/src/impls.rs`; `crates/dz_check/src/check.rs` (`"copy" => "Copy"`) |
| A by-value read: `DZ0403`, `DZ0402`, `DZ0401` | `crates/dz_flow/src/lib.rs`: `value`, `read` |
| `move` in the lexer, parser and tree | `crates/dz_lex/src/token.rs`, `crates/dz_parse/src/expr.rs`, `crates/dz_ast/src/lib.rs` (`ExprKind::Move`) |
| How a move and a placed copy lower | `crates/dz_cpp/src/lib.rs`: the `Move` lowering, `copy_tag` |
| The rules | `docs/spec/06-value-semantics-ownership-copy.md`; D-0106, D-0033 |
