---
id: T-0232
title: A Box of an abstract type is refused
milestone: M12
status: done
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
| The refusal, and the handles it allows | `lower_checked_type`, `abstract_by_value`, `is_prelude_handle` in `crates/dz_check/src/lib.rs`; `solve_call` in `check.rs` for `Weak<Shape>(…)` |
| The constructors that take `T` by value | `crates/dz_project/src/std/memory/box.dz`, `rc.dz` |
| A constructor of an abstract value not instantiated | `instantiate_type` in `crates/dz_mono/src/lib.rs` |
| Nor emitted | `ctor_takes_abstract` in `crates/dz_cpp/src/lib.rs` |

**Done 2026-11-17.** `Box` first, for `ecs`'s world (T-0172): an abstract type may be the
argument of a `Box`, the check no longer looks inside type arguments (each is checked where
it is written, so `Box<List<Shape>>` says so once, not three times), and an instance leaves
out a constructor taking an abstract value. Then `Rc` and `Weak` (`is_prelude_handle`),
written as a type or on a constructor call (`Weak<Shape>(&rc)`). `Rc<Shape>` needed one more
step: its constructor builds an `RcBlock<Shape>`, which holds a `Shape` by value, so
monomorphization no longer walks a constructor taking an abstract value, and nothing only it
uses is instantiated. `Box<Shape>(…)` of an abstract value is still refused, since no such
value can be written.

Tests: `a_box_of_an_abstract_base_holds_a_derived_value` and
`an_rc_of_an_abstract_base_holds_a_derived_value` (two derived types, a `Weak`, both modes,
`--track-memory`), in `dz/tests/run.rs`; in `typed/`, a `Box`, an `Rc` and a `Weak` of
`Drawable`; in `typeerrors/`, a `Box<List<Abstract>>` and an `Rc<[2]Abstract>`, which hold
abstract values.
