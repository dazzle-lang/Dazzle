---
id: D-0091
title: Ties: checked reference invalidation, and memory placed by the compiler
status: decided
---

**Decided: option A, all of it** (r81, 2026-10-27, by the user). References are checked
against the ties a call kills, every function declares its `kills` clause, and ordinary code
names no allocator: the compiler places memory. The syntax is T-0107's, as the user accepted
it: `@name` for a tie, `in` for where something points, `kills` a keyword. Spec: §6.7 (ties),
§14.6 (placement), revising §1.1, §5.2, §6.4, §14 and §14.1, §2.7 and §20; supersedes D-0010
and, in part, D-0047. Not yet implemented: T-0107 is the plan, in order, the checker first.

**Settled on the way, 2026-10-30 (r88, by the user):** every returned reference is tied.
With several view inputs and no `in` the signature must say which (an error, not a rule
picking `self`); `in (a, b)` names several; a reference, slice or `str` result with no view
input is an error, a raw pointer's is not; and a struct, tuple or optional holding a view
follows the same rules. A view only through a type parameter stays unchecked.

**The question.** Should Dazzle check that a reference is never used after the memory it
points into has moved or been freed, and, with that check in place, take the choice of
allocator out of ordinary code altogether?

**Why it is asked.** Two holes cause silent memory corruption today, and neither is checked
(§1.1, §6.4):

- **A reference into storage that a call moves.** `let n = counts.get_or_insert(k, 0)`,
  then `counts.insert(other, 1)` grows the table, and `n += 1` writes freed memory. The same
  happens with `as_str()` before `push_str`, or a `for` over a list that the loop pushes to.
- **A value used after its arena is released.** Something built in `Allocators::Temp`
  that outlives `reset()` is read from released memory (T-0104).

The regions prototype (T-0104, T-0105) tried to choose allocators automatically without a
checker, and each stage's safety rested on rules found by tests: copies through borrows,
elements popped out of a region container. A checker that knows when memory dies would make
any placement safe by construction, and would close the first hole whether or not placement
is automated.

**The proposal** (in full, with the scenarios it was checked against, in
[T-0107](../tasks/T-0107-design-ties-kills-clauses-compiler-placed.md)):

1. **Ties.** A tie is a span during which some memory is valid. A type declares named ties
   for the storage it owns (a `List` has `'items` and `'contents`), as public names for
   private fields. Every reference carries a tie: what it points into.
2. **`kills` clauses.** Every function says in its signature which ties it may end:
   `fn push(&mut self, v: T) kills self'items`, `fn spawn(world: &mut World, e: Enemy) kills
   world.spawned'items`. No clause means it kills nothing. The body is checked against the
   clause, and `dz fix` writes a missing one. Using a reference whose tie a call killed is an
   error.
3. **No allocator in ordinary code.** No allocator parameters, no `#allocator`, no
   `Allocators::Temp` in user code. The compiler puts each value in the most efficient place
   it can prove safe: the scope's region, wherever the caller wants the result, or (later) an
   arena owned by a container, else the heap. The `Allocator` trait stays as a std internal
   under `unsafe`.

**What it would re-decide.**

- §1.1, where memory safety is a non-goal: reference invalidation becomes checked. Raw pointers
  and `unsafe` stay unchecked.
- §6.4, "no lifetime system, by design". Ties are a lifetime system, but a light one: they
  never appear on structs that hold references, since D-0003 stays.
- D-0010 and §14.1, the allocator as an explicit parameter, and D-0047, `Allocators::Temp`
  in user code.

**What it keeps.** D-0003: references are never stored in fields. That is what keeps ties
off reference-holding structs, and the reason this is lighter than Rust's lifetimes. It also
keeps aliasing: `&mut xs[i]` beside `&xs[j]` stays legal, and a call is rejected only when
it can move the memory a live reference points into.

**The options.**

- **A. All of it**: ties, `kills`, and the compiler choosing every placement. Proposed.
- **B. Ties and `kills` only**, with allocators explicit as today. This closes the first hole and
  leaves the second; it can be done first and stands on its own.
- **C. Neither**: keep §1.1 and §6.4 as they are, and finish the regions prototype as an
  optimisation behind a flag.

A is B plus placement, so B is the first step either way. Deciding A means a §21 revision
row, superseding D-0010 and D-0047, and revising §1.1, §6.4 and §14.
