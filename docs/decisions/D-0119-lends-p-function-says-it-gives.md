---
id: D-0119
title: "lends p: a function says it gives a parameter to a parallel block"
status: decided
---

**Decided, 2026-11-25, r152: spec §22.12**, with §2.7, §20 and §22.6. The user's choice,
including the spelling.

**The question.** A region is a bump allocator one thread uses at a time (§14.6). Memory is
placed by the function that makes it, so a container made in `main` and handed down to a
function with a `parallel` block was placed without seeing the block: two of its calls could
grow two lists in `main`'s region at once. Placement cannot see it from either end: `main`
sees an ordinary `&mut`, and the function with the block cannot move what is not its own.

**What was tried first (r151, T-0265).** A runtime guard: while any block ran, every change
to a region or an owner arena through an allocator handle took a lock, first one for every
allocator, then one per arena. It was correct, but the check alone, a load and a branch per
allocation, cost up to 8% (`words` on g++) in programs that ran no block, and was then emitted
only for a program with one, which still paid it everywhere.

**The options put to the user.** The same information, "this function lets a block grow what
`w` reaches", carried to whoever places `w`: written in the signature, inferred by the compiler
across functions and modules, or found at run time by the lock. `&mut` alone cannot say it,
since nearly every `&mut` is single-threaded, and neither can `kills`, which says the same for
a call that grows `w` on one thread. The user chose the written clause, and its spelling,
`lends w`, without "to threads": the keyword means that alone.

**What it is.** `lends p` names a whole parameter, `&mut`, `&mut self` or a view, whose memory
a call in a `parallel` block may change and grow, here or through a function that `lends` it.
Held to the body as `kills` is, written by `dz fix`, carried up through callers; only what may
own memory counts. A function that lends is never a function value, since a function type has
no `lends`; an implementation or an override lends only what the method it implements does.
Callers put what they give there on the heap, so no region is grown by two threads and the
runtime check, the locks and each arena's lock word are gone.

**Open.** `lends` on a function type, which would let a lending function be a value; until
then it is called by name.
