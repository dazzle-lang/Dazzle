---
id: T-0232
title: A Box of an abstract type is refused
milestone: M12
status: open
kind: feature
area: stdlib
where: `crates/dz_check/src/check.rs`: the abstract-by-value check (`DZ0545`); `crates/dz_project/src/std/memory/box.dz`: `Box(value: T)`; `crates/dz_project/src/std/memory/rc.dz`: `Rc(value: T)`
---

Found 2026-11-14 while testing T-0229. `List<Box<Shape>>`, where `Shape` has a `virtual fn`
with no body, is refused with `DZ0545` ("`Shape` is abstract, so there is no value of it"),
at the type itself and at `Box<Shape>::size`. That is the main way a box of a base is used:
a list of UI elements, each a `Box<UiElement>` holding a `Button` or a `Label`, where
`UiElement` declares `draw` and defines nothing. The checker sees `Box<T>`'s constructor
take a `T` by value, and an abstract `T` cannot be one.

**What it needs.** A `Box<Abstract>` is never built from an `Abstract`: only converted from
a `Box<Derived>` (§14.5, D-0104, which allows it since an abstract type has a `virtual`
method). So the instance must exist without its by-value constructor: the checker accepting
an abstract type argument to `Box` and `Rc` (and `Weak`) when nothing calls the constructor
with it, and the emitter leaving that constructor out of the instance, as T-0199 leaves out
methods nothing calls (see T-0230 for that mechanism's limits). A test: `tests/run` with a
`List<Box<UiElement>>` of two derived types, dropped and freed whole under
`--track-memory`, and the `typed/` and `typeerrors/` pair (an abstract `Box(…)` still
refused).

## Where the code is

| What | Where |
|---|---|
| The refusal | `crates/dz_check/src/check.rs`, `DZ0545` |
| The constructors that take `T` by value | `crates/dz_project/src/std/memory/box.dz`, `rc.dz` |
| Which members an instance has | `crates/dz_mono/src/lib.rs`, `Called` (T-0199) |
