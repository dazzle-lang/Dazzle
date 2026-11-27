---
id: T-0011
title: Threads, a mutex, atomics and a job system
milestone: M19
status: open
kind: feature
area: concurrency
---

**Scheduled for 0.3 by the user on 2026-11-18, as M19.** The design in
[D-0096](../decisions/D-0096-threads-safe-multithreading-built-on-ties.md) was settled
in r144 after [T-0245](T-0245-try-break-d-0096-before-deciding.md). Spec §22.4 calls
for atomic counts on `Rc` and `Weak` in parallel programs. Thread-local drops on workers
(T-0247), synchronized memory tracking (T-0038), and safe `TypeMap` storage for the
ECS (T-0246) are implemented. T-0248 tracks ThreadSanitizer in CI. The notes below
record the earlier plan.

**Progress.** Atomics are done (`Atomic<T>`, D-0113, §22.9, r146), and so is the mutex
(`Mutex<T>`, §22.10, r147; its API is proposed in D-0114), and so is `spawn` with `Thread`
(§22.11, r150; D-0117, proposed), safe with `Send`, `Share` and `#[main_thread]` (T-0264),
and so is `parallel { … }`, which replaced `join!` (§22.3, r151; D-0118): calls checked
against each other, a thread per call, `Rc` crossing with atomic counts since r157 (T-0266), no results yet (T-0267), and
`lends p` for a container lent to a block from further out, so its caller puts it on the
heap (§22.12, r152; D-0119, T-0265), and so is **the parallel loop, `parallel for`** (§22.3,
r153; D-0120), over a range, an array, a slice or a `List`, through `std`'s
`ParallelIterate`: a thread per core per loop, the body checked to write only its element and
leave only by `continue`, what a pass makes on the heap, and the container it is given as
`&mut` lent, so `lends` covers it.
And so is **the pool of workers** (r154): a block's calls and a loop's ranges run on workers
that stay, one per core but the caller's, started on first use and joined at exit so their
`#[thread_local]` statics drop before the memory report. A loop run 2000 times over 256
elements took 1.2 s with a thread per range and 65–90 ms on the pool (20 cores, debug build).
**Left in the loop:** a consuming `parallel for` (refused, `// deferred: T-0011`); a pass's
own region from the worker's pool, where it is on the heap now; a query keyed by type looped
over in parallel, which waits on §22.8; per-pass output (D-0096's hole 4).
**Left of the job system:** jobs a program hands the pool itself, with dependencies between
them, beyond a block and a loop: not designed, and item 4 below (`std` or a library) is
the user's call. **`spawn!` is not built:** the user chose type parameter packs instead
(D-0121, r155, T-0269), so `spawn` itself takes any number of arguments.

## Where the code is

| Piece | Location |
|---|---|
| `Atomic<T>` and its native operations | `crates/dz_project/src/std/sync/atomic.dz`; `crates/dz_cpp/src/runtime.rs` (`NATIVE_H`, `NATIVE_CPP`); the static exception is `is_static_initialiser` in `crates/dz_check/src/check.rs` |
| `Mutex<T>` and its native lock | `crates/dz_project/src/std/sync/mutex.dz`, `mutex_guard.dz`; `crates/dz_cpp/src/runtime.rs` (`mutex_lock`, `mutex_unlock`) |
| Static lifetime and generated entry hooks | `crates/dz_cpp/src/lib.rs` (`static_decl`, `thread_statics`) and `crates/dz_cpp/src/runtime.rs` (`register_thread_cleanup`) |
| `spawn` and `Thread` | `crates/dz_project/src/std/sync/thread.dz`; `crates/dz_cpp/src/runtime.rs` (`thread_start`, `thread_join`, `thread_detach`); `-pthread` in `crates/dz/src/toolchain.rs` (`link`) |
| Reference counts and memory tracker | `crates/dz_project/src/std/memory/` (`rc_block.dz`, `rc.dz`, `weak.dz`, `tracker.dz`) |
| Type-keyed storage and milestone proof | `crates/dz_project/src/std/collections/type_map.dz` and `tests/bench_programs/ecs` |
| `parallel { … }`: syntax | `crates/dz_lex/src/token.rs` (`Parallel`); `crates/dz_parse/src/stmt.rs` (`parse_stmt`); `StmtKind::Parallel` in `dz_ast` and `dz_hir` |
| `parallel { … }`: one call each, main thread, `Send`/`Share` | `crates/dz_check/src/check/parallel.rs`; `parallel_call`/`parallel_needs` in `needs_main_thread` (`check.rs`); `holds_rc` in `threads.rs` |
| `parallel { … }`: calls against each other | `crates/dz_flow/src/ties/body.rs` (`record_parallel`, `parallel_conflicts`, `moved_from`); `report.rs` (DZ0424, `call_name`) |
| `parallel { … }`: placement, and `lends` | `crates/dz_flow/src/placement.rs` (`parallel_call`, `lent_shared_to`, `lends_anything`, `Cx::lent`); the rest of `lends` is in T-0265's table |
| `parallel { … }`: emitted C++ and threads | `crates/dz_cpp/src/lib.rs` (`Emitter::parallel`, `hold_args` in `call`, `CapturedArgs::hold`); `crates/dz_cpp/src/runtime.rs` (`ParallelJob`, `parallel_job`, `parallel_run`) |
| `parallel for`: syntax | `crates/dz_parse/src/stmt.rs` (`parse_stmt`, `parse_for`); `ForStmt::parallel` in `dz_ast`, `StmtKind::For { parallel }` in `dz_hir`; `dz_fmt` and both dumps |
| `parallel for`: what splits, and `ParallelIterate` | `crates/dz_project/src/std/sync/parallel_iterate.dz`; the impls in `collections/slice_iter.dz`, `slice_iter_mut.dz`; `parallel_subject` in `crates/dz_check/src/check/parallel_for.rs`; `want_impl` for it in `crates/dz_mono/src/lib.rs` |
| `parallel for`: the body (DZ0430, DZ0431, DZ0433, DZ0598, DZ0599) | `crates/dz_check/src/check/parallel_for.rs` (`parallel_write`, `parallel_body`, `reads_written`, `parallel_elements`, `leaves_pass`); called from `for_stmt`, `require_mutable`, `require_writable`, `require_mutable_receiver`, `loop_jump`, `Return`, `needs_main_thread_as` in `check.rs`; `LoopFrame::parallel`, `Body::parallel_loops` |
| `parallel for`: placement and `lends` | `crates/dz_flow/src/ties/body.rs` (the `For` arm, `lend_to_thread`); `crates/dz_flow/src/placement.rs` (`Cx::passes`, `pass_of`, `lends_anything`) |
| `parallel for`: emitted C++ and threads | `crates/dz_cpp/src/lib.rs` (`Emitter::parallel_for`); `crates/dz_cpp/src/runtime.rs` (`parallel_for`, `parallel_range`, `core_count`) |
| The pool of workers | `crates/dz_cpp/src/runtime.rs`, `NATIVE_CPP` (`JobGroup`, `pool_start`, `pool_worker`, `pool_take`, `pool_run`, `pool_stop`, `parallel_run`); tested by `tests/run/workers` |

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

- **[T-0001](T-0001-arc-t.md) `Arc<T>`** (dropped 2026-11-25: `Rc` itself gets atomic
  counts, [T-0266](T-0266-rc-or-weak-in-parallel-block.md)) — the atomically counted `Rc`. Its API is already
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
