---
id: T-0246
title: A safe TypeMap and owned trait objects, for a world keyed by type
milestone: M19
status: done
kind: feature
area: stdlib
where: `crates/dz_project/src/std/collections/type_map.dz` (`TypeMap`); `tests/bench_programs/ecs/libs/ecs/src/world.dz` (`World`)
---

A prerequisite D-0096 names ("Known problems"): without it, an ECS world keyed by component
type is `unsafe` inside, as the Odin one is.

**What exists.** A `Box`, an `Rc` and a `Weak` of an abstract base work (T-0172's ECS,
T-0232), so a world can already hold `List<Box<AnyStorage>>`.

**What is missing.** A map from a type to that type's storage, `TypeMap`, whose lookup
returns the storage at its real type through a **checked** downcast, so `m_sets.get<T>()` is
safe code. D-0096's per-type tie, `tie @[T] = m_sets[T]`, is written over such a map, and
`std` promises it gives each type its own storage.

**Done when** `ecs`'s world holds its sets in a `TypeMap` and its query reaches them with no
`unsafe` in the world itself.

**Done.** `TypeMap` owns entries through the existing abstract-base `Box` and verifies
each stored type before a downcast. It provides typed lookups and stable insertion
slots. `World` uses typed lookups for queries and walks registered storage removers
in slot order for despawn.
