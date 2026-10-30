---
id: T-0117
title: Memory placed by the compiler: regions and the result placement
milestone: none
status: open
kind: feature
area: ownership
---

Step 4 of D-0091's plan (spec §14.6), split out of
[T-0107](T-0107-design-ties-kills-clauses-compiler-placed.md), which has the full design and
the scenarios it was checked against.

**What it is.** No allocator appears in ordinary code: the compiler puts each value in the
most efficient place it can prove safe, the heap last.

| The value | Its memory |
|---|---|
| never leaves its scope | that scope's region |
| returned | wherever the caller wants the result (a hidden result placement) |
| anything else | the heap |

A value's owned parts live where the value lives; a constructor and `copy` build into
wherever `self` is placed. Every function returning something that owns memory takes a
hidden result placement — free functions, methods, trait methods and function values alike.
Where a by-value parameter ends up is inferred within a module; across modules, and through
anything the compiler cannot see, the default is the heap. With the ties checker, a wrong
placement fails the check and falls back to the heap.

**Why it matters.** It removes the class of bug where a value outlives its arena, and the
choice that is easy to get wrong (T-0099: `Temp` was slower than the heap for a growing
container). In the prototype, scope regions and result placement made `astar` 12–17% faster
than the heap.

**Why it is this way.** The regions prototype ([T-0104](T-0104-prototype-compiler-chooses-where-memory-comes.md),
[T-0105](T-0105-placement-follows-value-destination-not-function.md)) placed memory with no
checker, and each stage's safety rested on rules a test had found; ties are that checker.

**What it would take.** Stages 1 and 2 of the prototype re-based on ties, the result
convention for every function, constructors and `copy` building into `self`'s placement, and
what `dz` shows for each placement (inlay hints and a listing). To settle first: whether the
prototype's stage 3 on this branch is reverted now or replaced here. Should follow D-0093's
steps, since placement reasons about the same views. Then
[T-0118](T-0118-skip-drops-that-only-free-region.md) and
[T-0119](T-0119-remove-allocators-from-ordinary-code.md).
