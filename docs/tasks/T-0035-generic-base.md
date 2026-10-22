---
id: T-0035
title: A generic base
milestone: M12
status: done
done: r72, 2026-10-22
kind: deferral
area: checker
where: `dz_check/src/lib.rs`, `collect_base`
---

**What is refused.** Inheriting from an *instantiation* of a generic struct:

```dz
struct Panel<T> { value: T }
struct Button : Panel<i32> { … }        // DZ0541
```

`collect_base` reports `DZ0541`. The other direction works: a generic *derived* type over a
concrete base — `struct Slider<T> : Panel { … }` — is fine, and so is plain inheritance,
which landed at r30.

**Why it is refused.** Monomorphization lays out one struct per set of type arguments
(§8.4). A base is laid out *inside* the derived type, so `Button : Panel<i32>` would need
the layout of one particular instance of `Panel` to exist before `Button`'s own layout is
known, and `dz_mono` has no step that resolves a base's instantiation before fixing the
derived type's layout. The pieces each exist — instances are laid out, bases are laid out —
but not in that order.

**Why a game wants it** (from the readiness review, 2026-09-14). This is the shape UI and
component hierarchies take: a `Panel<T>` holding a value, with `Button`, `Slider` and
`Label` each inheriting a concrete instance of it. Today the choice is to make every derived
type generic too and thread the parameter through (`struct Button<T> : Panel<T>`, which
spreads a parameter nobody wanted into every signature), or to abandon the base and compose
instead.

**What it would take.** `collect_base` would accept an instantiated path and record the
base's type arguments on the symbol; `dz_mono` would need to request the base instance
before the derived type's layout, i.e. an ordering over instance layouts rather than the
flat set it collects now. Worth checking first whether the generated C++ needs anything at
all here — `struct Button : dz::Panel__i32` is valid C++ as soon as `Panel__i32` is emitted
first, so this may be entirely an emission-order problem rather than a layout one.

Related: [T-0036](T-0036-several-constructors-are-not-resolved-by.md) (constructors not
resolved by argument), which inheritance made visible in a second place.
