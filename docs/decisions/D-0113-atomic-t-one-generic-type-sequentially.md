---
id: D-0113
title: "Atomic<T>: one generic type, sequentially consistent for now"
status: decided
---

**Decided by the user on 2026-11-23**, when T-0011 reached atomics, which §22 required
without giving them an API. In the spec as §22.9, r146.

| | |
|---|---|
| **Shape** | One generic type, `Atomic<T>`, for `bool` and the integers, rather than `AtomicU32`, `AtomicBool` and so on. It matches `List<T>` and `Box<T>` and adds one name to `std` |
| **Memory order** | Every operation is sequentially consistent. Explicit orderings, as Rust and C++ have, are left for later, to keep the first version simple and prove the concept. Adding them later breaks no program |
| **A static** | `static NEXT_ID: Atomic<u32> = Atomic<u32>(0)`, the spelling the user chose. A static is otherwise built from constants and literals (§7.7), and `Atomic`'s field is private, so `Atomic<T>(constant)` is a named exception |

**What follows from it.**

- What an `Atomic` can hold is `std`'s to say: the bound is a private trait, so a program
  cannot implement it for a type no atomic instruction can write.
- `add` and `sub` exist only for integers, and return the value from before the operation.
- An `Atomic` is not plain data, so it is never copied by accident: two copies would be two
  counters. `clone` is explicit and reads the value once.

**Open.** Explicit memory orderings. `Rc`'s atomic counts (§22.4) are the compiler's own and
may use weaker orderings without waiting for this.
