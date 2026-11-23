---
id: T-0247
title: A thread_local static is dropped on every thread, not only the main one
milestone: M19
status: done
kind: feature
area: ownership
where: `crates/dz_cpp/src/lib.rs` (`static_decl`, `thread_statics`); `crates/dz_cpp/src/runtime.rs` (`BuiltStatic`, `register_thread_cleanup`)
---

D-0096 piece 6, and a prerequisite of threads: a `#[thread_local] static` (§7.7) may be
used on any thread, but only the main thread's copy is dropped. T-0163 made every static's
drop run at exit, in a fixed order, and left a dropped static at its default; that is for
the main thread. A worker that ends leaks its own copies, and one whose drop matters (a
file flushed, a region returned to a pool) does not happen.

**What it would take.** A worker thread drops its `thread_local` statics when it ends, in
the same order T-0163 fixed for exit, before the thread's storage goes. Test with a static
whose drop prints, read on two threads.

Note the MinGW g++ case already commented in `static_decl`: a `thread_local` with a
destructor links twice there, which is why the header declares it `extern`.

**Done.** The first access to a managed thread-local on a worker registers cleanup
and initializes the thread's managed statics in order. At thread exit it drops only
the worker's values, in reverse declaration and dependency order. The main thread
still drops its thread-locals and process statics in their original shared order.
`tests/run/thread_statics` checks both orders, separate values on two workers, and
allocation cleanup under `--track-memory`.
