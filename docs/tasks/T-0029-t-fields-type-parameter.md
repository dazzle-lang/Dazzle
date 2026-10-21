---
id: T-0029
title: `T::fields` of a type parameter
milestone: M10
status: open
kind: deferral
area: checker
where: `dz_check/src/check.rs`, the `T::fields` query
---

**What is missing.** Iterating a *type parameter's* fields at compile time:

```dz
fn dump<T>(value: &T) {
    #for f in T::fields { … }        // deferred
}
```

**Why it cannot be a macro.** Macros expand **before types exist** (§12.11, D-0074). That is
deliberate — it is what keeps expansion a syntactic operation and keeps macro errors pointing
at the invocation. So a macro cannot ask what `T`'s fields are; it has not been decided yet,
and in a generic function `T` is not one type at all.

§13.4 gives the mechanism that *does* work and is the answer for most of what people want
this for: a **per-field macro takes the declaration itself** and reads the fields from what
was written. `#serializable struct Transform { … }` generates a `Serialize` impl with the
fields written out, specialised and with no runtime lookup. That covers "generate code per
field for types I declare".

**What is genuinely missing**, then, is the other case: per-field code in a function generic
over a `T` the function does not declare. That is **not a macro** — it is a loop unrolled
once per instance, during monomorphization, when `T` is known. §13 already has the runtime
type table for the fully dynamic case (`TypeId`, reading and writing fields by name), so this
sits between the two: compile-time specialisation of code that was written generically.

**Why it matters.** This is what a generic serialiser, a generic editor-inspector or a
generic diff wants — `fn save<T>(v: &T)` that works for any struct without the author of `T`
having to opt in through `#serializable`. For a game that is the asset pipeline and the
debug UI. The workarounds are real, though: opt in with the §13.4 macro per type, or go
through the runtime table and pay the lookup.

**What it would take.** A `#for` over `T::fields` that `dz_mono` expands per instance, which
means a compile-time loop surviving into monomorphization rather than being resolved in the
checker — a new kind of deferred construct in HIR, and a decision about how far it can go
(a field's *type* is then known per instance, so the body can call type-specific methods,
which means the body is type-checked per instance and diagnostics have to point at the
written source, not the unrolling — spec principle 4 and invariant 5).

That is a large change, and worth weighing against simply making §13.4's declaration macro
the documented answer and saying so in §13.4 rather than leaving the deferral open-ended.
The user's call which.
