---
id: D-0114
title: "Mutex<T>: lock returns a guard that unlocks when dropped"
status: open
---

**Where it stands.** The user asked for the mutex on 2026-11-23. D-0096 requires `Mutex<T>`
and gives it no API, so what is built (§22.10, r147) is the smallest shape that fits the
language. **It is proposed, and the user has not yet chosen it.**

| | |
|---|---|
| **Taking the lock** | `m.lock()` waits, and returns a `MutexGuard<T>` |
| **Reaching the value** | `guard.get()` and `guard.get_mut()`. Dazzle has no `Deref`, so the guard cannot stand for the value as Rust's does |
| **Releasing it** | When the guard is dropped, at the end of its scope |
| **The guard** | A view struct, so it lives only in a local, a parameter or a result, never in a field or a container, and the mutex cannot move or drop while it exists |
| **What it gives out** | A reference tied to the guard, not the mutex, so it ends when the guard does |
| **Poisoning** | None. A panic on any thread ends the program (§22.3), so no thread can observe a value a panic left half-written |
| **Reentrancy** | None. Locking a mutex the thread already holds waits forever |
| **A static** | `static M: Mutex<T> = Mutex<T>(v)`, with `v` what a static may start as (§7.7) |

**Why not a closure**, `m.with(fn(v) { … })`, which would make a reference escaping the lock
impossible by construction: closures are not built (T-0259), and the guard's reference is
already checked.

**Open.**

- Whether this API is the one wanted.
- `try_lock`, which needs a guard in an optional, and a read-write lock.
- A self-deadlock is not detected.
