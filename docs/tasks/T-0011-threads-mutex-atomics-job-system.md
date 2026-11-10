---
id: T-0011
title: Threads, a mutex, atomics and a job system
milestone: v2
status: open
kind: feature
area: concurrency
---

**What this row is.** All of concurrency, in the order it has to be built: **atomics**, then
a **mutex**, then **threads**, then a **job system** on top of them. D-0011 places the whole
of it after the first release.

**Why after.** 0.1 is not finished until there is an LSP good enough to work in, a stdlib you
can write a game against, and C++ interop. Concurrency is a larger design surface than any of
those and it touches every part of the language that is already settled — ownership, drops,
the allocators, the memory tracker — so doing it early would mean revisiting decisions before
they had been used in anger. A single-threaded game is a real game; a game with threads and
the wrong ownership rules is not.

**Why a game eventually needs it.** Not for correctness but for the frame budget: physics,
animation and asset loading on other cores is what a modern engine does, and a job system is
how game code actually expresses it — not threads directly, but tasks with dependencies fed
to a pool. So the job system is the goal and the other three are the means.

**What it depends on, and what depends on it.**

- **[T-0001](T-0001-arc-t.md) `Arc<T>`** — the atomically counted `Rc`. Its API is already
  shaped for this; the row exists so that shape is not lost.
- **[T-0038](T-0038-memory-tracking-single-threaded.md)** — `TRACKER` is one static with no
  lock, so `--track-memory` races the moment two threads allocate. A trap for whoever adds
  threads, and part of this work rather than a prerequisite for it.
- **[T-0044](T-0044-hash-combine-relies-on-u64-arithmetic.md)** is *not* a dependency but is
  adjacent: it waits on what plain `+` does on overflow, which is settled (18.63, `-fwrapv`).

**The decisions that have to come first.** None of this is mechanical, and each wants a
decision file before code:

1. **What the language promises.** Since r113, §1.1 promises that safe code has no
   undefined behaviour **on one thread**, and names threads as where the promise ends. A
   data race is undefined behaviour, so threads either extend the promise (something like
   `Send`/`Sync`, with the aliasing §6.7 allows on one thread refused across two) or stay a
   stated exception, a data race being the programmer's problem as a `^T` in `unsafe` is.
   Either is defensible; it should be chosen deliberately rather than arrived at. A
   `thread_local` static on a thread other than the main one is not dropped yet (T-0163).
2. **What a thread may capture.** A `&T` cannot be stored in a field (D-0003) because
   lifetimes are not tracked; a closure handed to a thread is the same question with a
   harder answer.
3. **Where `Allocators::Temp` lives.** A per-frame arena is the right shape for a single
   thread and wrong for several; per-thread arenas with a reset barrier is the usual answer
   and changes §14's story.
4. **Whether the job system is `std` or a library.** It is the piece most likely to have
   opinions in it, and the one a game is most likely to want to replace.

So the realistic first step, when the time comes, is the decision file for item 1 — because
it determines how much of the rest is checking and how much is documentation.

**A direction, noted 2026-11-01; not a priority.** The user put threads behind regions.
[D-0094](../decisions/D-0094-safe-code-has-no-undefined-behaviour.md) proposes promising no
undefined behaviour on one thread. Its aliasing, a `&mut` beside a `&`, is sound only on
one thread, so threads cannot simply be added beside it. One shape keeps single-threaded
code exactly as free as it is:

- a value crosses threads only by `move`, and only if it holds no view, no `Rc` and no raw
  pointer, a property worked out as copyability is;
- `parallel for &mut e in xs { … }` gives each pass one element, and freezes everything
  else the body touches for the duration, checked as a `kills` clause is;
- `Rc` stays on one thread; `Arc` ([T-0001](T-0001-arc-t.md)), a mutex and atomics cover
  the rest.

That would answer item 1 with checking rather than documentation, and confine Rust's
discipline to the code that runs in parallel.

**The design, written 2026-11-10:
[D-0096](../decisions/D-0096-threads-safe-multithreading-built-on-ties.md)**, open. It
grows the direction above into a whole design, agreed in direction with the user: memory
another thread sees is owned, frozen or synchronised; `join`, `for_each` and `spawn` live
in `std`, not syntax; and a world keyed by component type gets a tie per type, so ECS
systems run in parallel with their access checked from their signatures. It answers
items 1–4 above, and lists the programs that must be refused before it is decided.
