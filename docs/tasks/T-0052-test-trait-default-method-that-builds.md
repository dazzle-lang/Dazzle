---
id: T-0052
title: Test a trait default method that builds a `Self`
milestone: M12
status: open
kind: deferral
area: generics
---

**The gap is in the tests, not the compiler.** A trait method may have a default body, which
every implementer inherits unless it overrides it. `tests/run/generics` exercises those
defaults — but only for bodies that call **the trait's own methods** on `self`:

```dz
pub trait Shape {
    fn area(&self) -> f32
    fn describe(&self) -> f32 { return self.area() * 2.0 }   // covered
}
```

A default body that **builds or returns a `Self` value** is not tested at all:

```dz
pub trait Zeroed {
    fn zero() -> Self
    fn twice() -> Self { return Self::zero() }               // not covered
}
```

**Why that is the case to worry about.** `Self` in a default body is the one place the body
is not generic over something it was given — it has to be resolved per implementer at
monomorphization, which means the default is instantiated once per implementing type with
`Self` substituted, and a constructor or a return by value then has to lay out a type the
trait itself knows nothing about. It is plausible this works; it is equally plausible it
emits a C++ `Self` that does not name anything, or picks the trait object's layout. Nobody
knows, which is the problem: an untested path in a monomorphizer is where a miscompile
hides, and a miscompile in generated C++ is a compiler bug by invariant 7, never a user
error.

Note also that a trait method taking no receiver cannot be called through a `^Trait`
([T-0049](T-0049-trait-objects-that-are-not-object.md)), so the test should cover the static
path and confirm the trait-object path is refused rather than quietly emitting something.

**What it would take.** A case in `tests/run/generics`: a trait with a `fn make() -> Self`
default that constructs a `Self`, two implementers with different sizes and one with a
`Drop`, and an `expected.txt` that distinguishes them. Small, and it either passes — in
which case the gap closes for the cost of writing it — or it finds something, which is the
point.
