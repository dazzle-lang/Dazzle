---
id: T-0242
title: A trait's default body cannot call a supertrait's method on self
milestone: M12
status: done
kind: bug
area: checker
where: `dz_check/src/lib.rs`: `self_trait_bounds`; `dz_check/src/impls.rs`: `implements`; `dz_check/src/check.rs`: `param_method` and the `via_supertrait` path in `call`
---

Found with T-0037 (2026-11-17). In a default body, `Self` is read as the trait's own type,
which implements that trait and nothing else. So its supertraits are invisible:

```dz
trait Tr {
    fn size(&self) -> i32
}

trait Counted : Tr {
    fn twice(&self) -> i32 {
        return self.size() * 2      // DZ0520: `Counted` has no method `size`
    }
}
```

The same holds for a bound: `Tally<Self>` where `Tally<T: Tr>` is `DZ0531`, "`Counted` does
not implement `Tr`". Every implementer of `Counted` implements `Tr` (spec §7.6), so both
should be accepted. A generic `fn f<T: Counted>(x: &T)` can call `x.size()`, through
`trait_closure`; the trait-as-`Self` case does not go through it.

**The fix to look at:** where `implements` says a trait satisfies itself, take the trait's
supertrait closure as well, and let method lookup on the trait's own type search its
supertraits. A test: the two snippets above check clean, and one calling a method of a trait
that is not a supertrait is still `DZ0520`.

**Done** (2026-11-17, r140). `self_trait_bounds` gives a trait's own type its trait, whose
closure includes what the trait requires. `implements` uses it, so `Tally<Self>` meets a
bound on a supertrait. `param_method` uses it, so `self.size()` in a default body reaches a
supertrait's method through the same path as a bounded parameter. That path is taken only for
`self` in the trait's own body: a trait object's vtable has its own trait's methods, so
`c.size()` through a `&Counted` is still `DZ0520`. Tests: `dz_check`'s
`a_default_body_reaches_its_supertraits`, and `tests/run/default_bodies`, which runs both.
