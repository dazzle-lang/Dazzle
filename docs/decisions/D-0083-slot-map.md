---
id: D-0083
title: SlotMap and Key in std
status: decided
---

**A general-purpose `SlotMap<T>` with typed `Key<T>`s in `std`, and no ECS; chosen by the
user on 2026-10-22.**

T-0004 had proposed a game-framed `Pool<T>`/`Handle<T>`. The user asked whether that
belongs in a standard library at all, since no mainstream language ships one and games use
frameworks such as flecs or EnTT. The answer settled on:

- **The container is general, the framework is not.** A stable, typed ID that reads as
  absent once its value is gone serves compilers, GUI trees, graphs and connection tables
  as much as games. An ECS — archetypes, queries, systems — stays out of `std` for good.
- **Dazzle has a reason the others do not**: D-0003 keeps references out of fields, so a
  struct that refers to something it does not own needs an ID. Rust's `slotmap` crate is
  popular for the same reason.
- **Named `SlotMap` and `Key`**, not `Pool` and `Handle`: §14.2 already promises a `Pool<T>`
  allocator, and `Handle` is what §14.5 calls `Rc`, `Weak` and `Box`.
- **A lookup is a pointer**, `get(k) -> ^T`, as `HashMap::get` is (§9.1). A key does not
  implement `Nullable` and cannot be resolved by `or` on its own, since it needs its map; no
  language change was wanted for it.
- **Dense storage**: the values in one array, removal moving the last into the gap, so
  iterating is a linear scan and `values()` is a slice. Order is not kept, as a `HashMap`'s
  is not.
- **Versions by parity**: odd while a slot holds a value, even while it is free, so a forged
  or null key (version 0) never names a free slot. After 2³¹ reuses of one slot the version
  comes round again; that is accepted rather than retiring the slot.

Spec: §14.4, §14.5.
