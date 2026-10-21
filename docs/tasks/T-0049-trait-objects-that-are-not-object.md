---
id: T-0049
title: Trait objects that are not object-safe
milestone: M12
status: open
kind: deferral
area: codegen
where: `dz_cpp`, `object_safe`; `dz_check`, `refuse_generic_impl_object`
---

**What is refused.** A `^Trait` is a trait object: a pointer plus a vtable built once per
implementing type. `object_safe` in `dz_cpp/src/lib.rs` says which traits can have one —
its methods must be callable knowing nothing but the vtable. Five shapes cannot, and each
reports `DZ0801`:

- a **generic trait** — `^Show<T>` would need a vtable per type argument;
- a trait with **associated types** — the vtable's signatures are not fixed until the
  associated type is;
- a trait with a **generic method** ([T-0007](T-0007-generic-methods-in-trait.md)), or one
  **taking no receiver** — neither has a single slot to occupy;
- a trait whose methods **mention `Self`** anywhere but the receiver —
  `fn clone(&self) -> Self` has no one return type across implementers;
- a trait object of a type whose **`impl` is generic** — `impl<T> Format for ^T`, where
  there is one implementation written but an unbounded number of instances, each needing its
  own vtable. This one is refused in the checker, by `refuse_generic_impl_object`.

**Why a game needs it** (from the readiness review, 2026-09-14). The first and the last are
the ones that bite. Generic traits are how Dazzle tells people to work around generic trait
methods ([T-0007](T-0007-generic-methods-in-trait.md)) — "make the trait generic instead" —
so a programmer following that advice loses the ability to hold the result behind a
`^Trait`, which is often why they wanted a trait at all. And the generic-`impl` case is the
reason a pointer cannot be formatted ([T-0055](T-0055-pointer-has-no-format.md)): the
blanket `impl<T> Format for ^T` has no vtable, so no formatting macro accepts a pointer.

**What it would take.** These are not one problem. The `Self`-mentioning case is a genuine
impossibility and should stay refused — Rust refuses it too, and the diagnostic should say
so rather than implying it is unfinished. The associated-type case follows
[T-0050](T-0050-associated-type-projection.md). The first and the last are the same problem
as [T-0007](T-0007-generic-methods-in-trait.md) option 2: a vtable per (type, instantiation)
pair, which means `dz_mono` deciding vtable layout as a whole-program property and so
fighting the per-module caching invariant 3 depends on.

So the realistic step is **not** to lift the refusal but to give each reason its own
diagnostic. `DZ0801` is `CODEGEN_UNSUPPORTED` — a general "the backend cannot do this yet"
code shared with unrelated limitations — so a programmer who hits it cannot tell whether
they wrote something impossible, something unfinished, or something unrelated to trait
objects at all. That is the real defect here, and it is worth fixing before any of the
harder work: a code per shape, each naming the shape and what to do instead. Diagnostic
codes are never reused (invariant 4), so this is new codes and a `tests/errors` file each.
