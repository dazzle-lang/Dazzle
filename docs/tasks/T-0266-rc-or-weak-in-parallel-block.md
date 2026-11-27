---
id: T-0266
title: An Rc or a Weak in a parallel block: atomic counts, checked by type
milestone: M19
status: done
kind: feature
area: concurrency
---

**Done 2026-11-27 (r157).** §22.4 lets `Rc<T>` and `Weak<T>` cross into a parallel loop and a
`parallel` block, checked as one place per `T`, with counts that are atomic in any program that
runs code at the same time. The block refused them until then (DZ0599), the user's choice on
2026-11-24 (D-0118), since the counts were plain integers. What was built, and why each piece
is shaped as it is, is in `docs/log/m19-threads.md`.

- **The counts.** `std`'s `Rc` and `Weak` count through four `extern fn`s, inline in the
  runtime's header: plain integers, or atomics when `dz_native.h` says `DZ_PARALLEL 1`, which
  the compiler writes for a program with a `parallel` block, a `parallel for` or a call of
  `spawn` anywhere in it (`runs_in_parallel`). The `Rc`s together hold one share of `weak`,
  so whichever of the last `Rc` and the last `Weak` goes second frees the allocation, and a
  `Weak`'s guard takes a count only if it is not zero.
- **What crosses.** Into a block or a loop (not `spawn`), a handle crosses as its value does:
  `&` asks `Share` of `T`, `&mut` and by value `Send`, through `std`'s own `impl Send for
  List<T>` too.
- **Between calls.** An argument that reaches an `Rc<T>` reaches the one place every `Rc<T>`
  stands for, written unless lent as `&` (DZ0424), and so does a view taken through one.
- **In a loop.** A pass given its element as `&mut` or by value is given no handle (DZ0599);
  a pass reads no `Rc` whose value may hold the container the passes write (DZ0433).

**Left:** a `#[thread_local]` static holding an `Rc` a parallel call put there would alias a
value another call writes; putting it there takes `unsafe`, which vouches for it, so it is not
checked. Skipping the guard's count where nothing can release the handle is T-0002.

## Where the code is

| Piece | Location |
|---|---|
| The counts in `std` | `crates/dz_project/src/std/memory/rc_block.dz` (the `rc_count_*` extern fns, `RcHeader`), `rc.dz`, `weak.dz` |
| The counts in C++ | `crates/dz_cpp/src/runtime.rs`, `NATIVE_H` (`DZ_PARALLEL`, `rc_count_*`); `native_h` and `handle_upcast_ctor` in `crates/dz_cpp/src/lib.rs`; `extern_fn` there writes every shell `inline` in `std`'s header |
| Whether a program runs anything at once | `runs_in_parallel` in `crates/dz_check/src/threads.rs`; read by `emit_selected` (`crates/dz_cpp/src/lib.rs`) and by `Facts::of` in `crates/dz/src/reuse.rs`, so a change redoes every module |
| What crosses (DZ0599) | `parallel_crossing_blocker`, `HANDLES_CROSS`, `named_crosses`, `crossing_blocker` (`Blocker::in_value`) in `crates/dz_check/src/threads.rs`; called from `crossing` in `check/parallel.rs` and `pass_crossing` in `check/parallel_for.rs`; `elements_share_handles` there |
| What a call reaches through a handle (DZ0424) | `crates/dz_flow/src/ties/body.rs`: `record_parallel`, `handles`, `Access::shared`, `parallel_conflicts`, `meets_shared`; `handles_reached`, `handles_meet` in `crates/dz_check/src/threads.rs`; `parallel_handle_conflict` in `ties/report.rs` |
| A pass reading an `Rc` that may hold its container (DZ0433) | `crates/dz_flow/src/ties/body.rs`: `PassFrame`, the `Local` arm of `expr`, the `For` arm, `pass_handle_reads`; reported in `ties/report.rs` |
| Tests | `tests/run/parallel_rc`; `tests/errors/parallel_rc_*.dz`, `parallel_for_writes_rc_element.dz`, `parallel_for_in_rc_reads_rc.dz`, `spawn_an_rc.dz`; `typed_parallel_rc` in `tests/workspace/typed`; the `first_parallel_block` scenario in `crates/dz/tests/incremental.rs` |
