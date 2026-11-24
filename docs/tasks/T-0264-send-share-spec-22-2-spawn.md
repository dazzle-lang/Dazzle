---
id: T-0264
title: "Send, Share and #[main_thread]: the checks that make spawn safe"
milestone: M19
status: done
kind: feature
area: concurrency
---

**Done 2026-11-24 (r150).** `spawn` is safe; what was built is in `docs/log/m19-threads.md`.

## Where the code is

| Piece | Location |
|---|---|
| `Send`, `Share`, main-thread drops | `crates/dz_check/src/threads.rs` (`sendable`, `shareable`, `synchronises`, `dies_on_main_thread`, `crossing_blocker`); `implements` in `crates/dz_check/src/impls.rs` |
| `#[main_thread]` on bodies | `crates/dz_check/src/check.rs`: `note_static`, `note_main_thread_call` (from `check_args`), `note_main_thread_value`, `settle_main_thread`, `main_thread_ceiling`; DZ0596 in `check_impl` |
| On signatures and types | `FnSig::main_thread` (`fn_sig` in `crates/dz_check/src/lib.rs`); `Ty::Fn`'s `MainThread` and `assignable` in `crates/dz_hir/src/ty.rs`; the type syntax in `crates/dz_parse/src/ty.rs` |
| The attribute's targets | `attributes` in `crates/dz_resolve/src/walk.rs` |
| A recipe's `thread_safe` | `ThreadSafe` and `Trust` in `crates/dz_project/src/lib.rs` |
| `std`'s side | `crates/dz_project/src/std/sync/send.dz`, `share.dz`, `synchronises.dz`, `thread.dz`; the impls at the end of `list.dz`, `deque.dz`, `hash_map.dz`, `string.dz`, `mutex.dz`, `atomic.dz` |


`spawn` (§22.11, r150) is an `unsafe fn`, because nothing checks what it is given: a safe
`spawn` would let a program with no `unsafe` race, which §1.1 forbids. The user asked on
2026-11-24 that the safety held before threads be kept. This task builds the checks, and ends
by making `spawn` safe to call:

- **`Send` and `Share`** worked out from a type's fields, as copyability is (§22.2): both for
  numbers and plain structs; neither for a type holding a raw `^T` or `^mut T` that is not
  `own`, a view or an `Rc`; `Mutex`, the atomics and, later, channels declared by hand in
  `std` under `unsafe`. Bounds in generic code, `fn f<T: Send>(…)`, and `spawn<A: Send>`.
- **`#[main_thread]`** (§22.5): a function that touches a plain static, or calls C its recipe
  does not mark thread-safe, says so, carried up through its callers as `kills` is, and written
  by `dz fix`; such a function becomes a value only of a `#[main_thread] fn(…)` type, so it
  never reaches `spawn`.
- **Placement** of what a thread is given (§22.6): a container moved into `spawn` keeps its
  `m_allocator`, which may be the caller's region, so it must be placed on the heap.
- **`Rc` and `Weak` refused by `spawn`** (§22.4).
- **Then `spawn` stops being `unsafe`**: remove it from `std/sync/thread.dz`, from
  `tests/run/threads`, and the §22.11 note, and turn `tests/errors/spawn_outside_unsafe` into
  tests of what is refused.

`join!` and the parallel loop need the same rules.
