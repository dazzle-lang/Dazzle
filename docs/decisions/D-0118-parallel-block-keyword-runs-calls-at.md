---
id: D-0118
title: "parallel { … }: a block keyword runs calls at the same time, in place of join!"
status: decided
---

**Decided, 2026-11-24, r151: spec §22.3**, with §2.7, §12.8 and §20. The user's choice.

**The question.** D-0096 decided that parallelism is `std`'s, not syntax, and spelled the
group of calls that run at once `join!(a(…), b(…))`, a macro of `std`. With no closures
(T-0259), such a macro cannot carry each call's arguments to another thread, the reason
`spawn!` waits too, so `join!` would have been the compiler's own macro, as `format!` is: syntax
under a macro's name. Asked how to define it, the user answered with a block keyword instead:

```dz
parallel {
    physics(&mut w.positions, &w.velocities)
    animate(&mut w.skeletons, &w.time)
}
```

**The spelling.** `run` was the user's first word for it, but 43 `.dz` files name something
`run` (`fn run(…)` in tests, benchmarks and the parse corpus), so reserving it would break
programs; `parallel` appeared only in two comments, and says what the block does.

**What the block is.**

- Each statement is one call to a function or a method, returning nothing (DZ0597). Results
  are not handed back yet, the user's choice for this step (T-0267).
- Every argument, receiver and default is evaluated first, in order, on the thread running the
  block; then the first call runs there and each other on a thread of its own, the user's
  choice over a persistent pool, which comes with the job system (T-0011); the block ends when
  all have. A thread that cannot start runs its call after the first.
- No call may need the main thread (DZ0598), `unsafe` or not; what crosses is held to §22.2
  (DZ0599); and two calls to §22.3's rule (DZ0424).
- `Rc` and `Weak` are refused for now, the user's choice, until their counts are atomic
  (T-0266, §22.4).

**What it changes.** `join!` is gone from §22; `parallel` is a keyword (§2.7), a statement
(§20), and needs no `#[parallel]` (§12.8). D-0096's "parallelism is library, not syntax"
still holds for the parallel loop and `spawn`.

**Placement under it (§22.6).** What a call is lent `&mut` or given by value, and what is made
for it, goes to the heap when it is the body's own; a container lent from further out may be
in a caller's region. At first (r151) a runtime lock guarded that case; since r152 the
function that gives such a container to a block says `lends` (D-0119), its callers put it on
the heap, and nothing is locked.
