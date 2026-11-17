---
id: T-0238
title: A virtual method with a body cannot promise what its overrides kill
milestone: M15
status: open
kind: design
area: ownership
---

Found 2026-11-17 writing `ecs`'s library (T-0172). The world keeps each component's storage
behind a base, `AnyStorage`, with one `virtual fn remove(&mut self, entity: Entity)`, which
each `Storage<T>` overrides to take the entity out of its lists: `kills self@items,
self@values`. A call through the base must therefore be allowed to kill something.

Spec §6.7 makes a clause a promise only "where there is no body to hold it to": a trait
method without a default, an `extern fn`, a function type. A base method with a body is
held to it like any other, so `pub virtual fn remove(&mut self, _entity: Entity) kills self
{}` is refused (`DZ0416`, "`remove` never kills `self`"). The only way to write it is with no
body, which makes `AnyStorage` abstract. That worked here, once a `Box` of an abstract base
was allowed (fixed with T-0172), but it forces every base whose overrides kill something to
be abstract, and gives a base no way to have a default that kills less than an override may.

**The question for the user.** Should a `virtual` method's clause be a promise, as a trait
method's is, with the base's own body checked against it as an implementation's is (no more
than it promises, as `DZ0573` says of a trait's)? A trait method with a default has the same
shape, and whatever is decided should hold for both. Today a `virtual` method's clause is
exact for its own body; under a body-less base's `kills self`, an override's `kills
self@items, self@values` was accepted. Whether an override that kills more than its base
promises is refused was not checked.

| What | Where |
|---|---|
| Clause checks: a path the body never kills (`DZ0416`) | `crates/dz_flow/src` (the `kills` checks), `crates/dz_check` |
| An override's clause against the base's | where `override` is checked, `crates/dz_check/src/check.rs` (`BAD_OVERRIDE`) |
| The case | `tests/bench_programs/ecs/libs/ecs/src/storage.dz`, `AnyStorage::remove` |
