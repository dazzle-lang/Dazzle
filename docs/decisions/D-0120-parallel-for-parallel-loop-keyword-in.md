---
id: D-0120
title: "parallel for: the parallel loop is a keyword in front of for"
status: decided
---

**Decided, 2026-11-25, r153: spec §22.3**, with §10.4 and §20. The user's choice of spelling.

**The question.** D-0096, as amended on 2026-11-18, wrote the parallel loop as a `for` over a
parallel iterator a container hands out: `for &mut e in enemies.parallel()`, and
`w.query_parallel<…>()` for a query. D-0118 then made `parallel` a keyword for the block, so
the method could neither be called nor declared under that name. Asked how to spell it, the
user answered with the keyword itself: `parallel for &mut e in enemies { … }` ("or `for
parallel`"). The keyword comes first, as it does for the block.

**What it is.** `parallel for` loops over what `for` would, with `for`'s head, bindings and
all (§10.4): a range, an array, a slice, or a container through the iterator it hands out,
`iter()` or `iter_mut()`. That iterator must implement `std`'s `ParallelIterate`, which
`List`'s do. The passes are cut into one range per core, in order, each walked by its own
thread; the statement ends when every pass has. A pass count from `indexed()` counts the
element's place in the whole loop, since the ranges are cut in order. §22.3's rules for the
body are unchanged: each pass writes only its element, reads only what is `Share` from
outside, and leaves only by `continue`.

**What it changes.** There is no `.parallel()` method and no `query_parallel`: a query is
looped over in parallel as any container is, `parallel for _, &mut p, &v in w.query<…>()`,
once its iterator implements `ParallelIterate` (§22.8, not built yet). Writing `parallel` in
front of a `for` is the whole conversion of a single-threaded loop.

**Proposed with it, Claude's, open to change:** `ParallelIterate`'s two methods. `passes()`
counts what is left, and `part(start, end)` makes an iterator of the same type over those
passes alone, from `&self`, so every part is made at once, one per thread. The promise that
two parts over ranges that do not overlap share no element is the implementation's, kept
inside `unsafe` where it hands out `&mut`.
