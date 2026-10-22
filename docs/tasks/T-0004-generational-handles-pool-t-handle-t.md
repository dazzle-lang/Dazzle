---
id: T-0004
title: A slot map: `SlotMap<T>` and `Key<T>`
milestone: M12
status: done
kind: feature
area: stdlib
---

**Done in r73 (D-0083), 2026-10-22.** Written first as a game-framed `Pool<T>` handing out
`Handle<T>`s. The user asked whether that belongs in a standard library at all, since game
code gets it from frameworks such as flecs and EnTT. The answer recast it as a
general-purpose container and left the ECS out of `std` for good.

**What it is.** A `SlotMap<T>` keeps values in one contiguous array and hands out a
`Key<T>`: a slot index and that slot's version, eight copyable bytes. Removing a value moves
its slot's version on, so a key to something that has gone reads as **absent**: not a
crash, and not whatever occupies the slot now.

```dz
let mut enemies = SlotMap<Enemy>()
let k = enemies.insert(Enemy{ hp: 10 })      // 0v1
let _ = enemies.remove(k)                    // slot 0 is now at version 2
let e = enemies.get(k) or return             // null: 1 != 2
```

**Why it is in `std`.** D-0003 keeps references out of struct fields. So a struct that
refers to something it does not own (an entity, a graph node, a connection) needs either
an `Rc`/`Weak`, which costs a refcount and a heap allocation per value, or an ID. A typed
ID that notices when its value has gone is that ID. Compilers, GUI trees and connection
tables want it as much as games do.

**What was settled** (D-0083):

1. Named `SlotMap`/`Key`: §14.2 already promises a `Pool<T>` allocator, and §14.5 uses
   "handle" for `Rc`, `Weak` and `Box`.
2. Resolved through the map, `get(k) -> ^T`, as `HashMap::get` is. `Key` does not
   implement `Nullable`, so no language change was needed.
3. `Key<Enemy>` is not `Key<Bullet>`. A type parameter the struct stores no value of needed
   no checker work.

**Where it is.** `crates/dz_project/src/std/collections/slot_map.dz`, `key.dz`,
`slot_map_iter.dz` and `slot_map_iter_mut.dz`; spec §14.4, with a pointer from §14.5; tested
by `tests/run/slot_map` and a case each in `typed/` and `typeerrors/`. Building it exposed
a checker bug: `&` of a reference was typed as a reference to a reference. It was fixed in
the same revision (§5.2, `docs/log/m12.md`).
