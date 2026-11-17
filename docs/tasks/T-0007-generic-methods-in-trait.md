---
id: T-0007
title: Generic methods in a trait
milestone: M12
status: done
kind: feature
area: generics
where: `dz_check/src/check.rs`: `trait_method_call`, the bound and qualified call paths (each solves the method's own type arguments), `is_own_self` and `DZ0589`, `signature_differs` (type parameters by position); `dz_mono/src/lib.rs`: `ImplMethodInstance`, `want_impl_method`, `instantiate_impl_method`, `impl_default`; `dz_cpp/src/lib.rs`: `impl_method_instance_fn`, `object_safe`, `trait_methods`; `tests/run/trait_generic_methods/`
---

**What is refused.** A method of a *trait* may not have type parameters of its own. The
checker reports `DZ0557` at the parameter (`refuse_generic_method` in
`dz_check/src/check.rs`), with the reason: a trait's method is called through
implementations the caller may not know, and through a `^Trait` with no type at all.

```dz
pub trait Writer {
    fn write<T: Format>(&mut self, value: T)     // DZ0557
}
```

A `virtual` method is refused for the same reason, said differently: one vtable slot cannot
hold an instance per type argument. Generic methods on a plain `impl` are fine — it is only
the dispatched ones that cannot be.

**Why it is refused, concretely.** Dazzle monomorphizes: `write<i32>` and `write<f32>` are
two different functions in the generated C++ (§8.4). A trait object is a pointer plus a
vtable of function pointers, built once per implementing type. So a generic trait method
would need either a vtable whose size depends on *which instantiations the whole program
uses* — computable only after monomorphization has seen every call site, which is a
whole-program step the compiler does not currently do in that order — or dispatch at run
time on a type, which the language has no mechanism for and does not want one.

**Why it matters.** The honest answer is: less than the other M12 rows, and the workarounds
are good. The three shapes people reach for are all expressible:

- **Make the trait generic** instead of the method: `trait Writer<T>` and
  `impl Writer<i32> for Log`. One implementation per type rather than one method per type,
  and the diagnostic already says to do this. This covers most cases.
- **Take the varying part as a trait reference**: `fn write(&mut self, value: &Format)`.
  This is what `Formatter` itself does — `std/core/formatter.dz`'s `write_args` takes
  `..&Format`, one indirect call per argument
  ([T-0056](T-0056-formatting-macro-arguments-go-through-format.md)) — and it is why the
  formatting machinery works today without this feature.
- **A macro**, where the per-type code is wanted at the call site (§12).

So this is a sharp edge rather than a blocker: it stops a `Writer` trait with a generic
`write`, and anything shaped like it, from being written the obvious way.

**The two possible answers, and what each costs.** This needs the user's decision before
any code:

1. **A rule that a `^Trait` cannot call a generic method.** The method is allowed in the
   trait and monomorphized per implementer for *static* calls (`log.write(3)` where `log`'s
   type is known), and calling it through a trait object is a compile error naming the
   method. Cheap to implement — the check moves from the declaration to the call site —
   and it is the same bargain Rust makes with object safety. The cost is that a trait's
   surface now depends on how you hold it, which is a new idea in this language and one
   more thing to explain.
2. **Per-instance vtables.** Monomorphization collects the instantiations each trait object
   needs and emits a vtable per (type, instantiation set). Keeps every trait callable
   through `^Trait`, at the cost of making vtable layout a whole-program property —
   which fights the per-module caching invariant 3 depends on, and makes a vtable's size
   depend on code in another module.

Option 1 is the one to build, if either is. Option 2 is a large change to `dz_mono` for a
feature whose workarounds are already idiomatic.

**It is a language change** — §7.6 and §8.1 currently imply the blanket refusal — so it
gets a `docs/decisions/D-NNNN`, a §21 revision row, the `DZ0557` message rewritten to say
the new rule, and matched `typed/` and `typeerrors/` cases (invariant 8), including a trait
object calling the method, which must still be refused under option 1.

Related: [T-0049](T-0049-trait-objects-that-are-not-object.md) (trait objects that are not
object-safe), [T-0050](T-0050-associated-type-projection.md) (associated-type projection).

**Done** (2026-11-17, r139): option 1, the user's choice ("a vtable would be bad for
performance"), recorded as D-0108, amending D-0066. A trait's method may have type
parameters of its own; each implementation has one function per type argument, made where
the receiver's type is known, and a call through a trait object is `DZ0589`, new. A
`virtual` method is still `DZ0557`.

- **Checker.** The trait's refusal is gone. Every path to a trait method solves the
  method's own type arguments, inferred or written: a concrete receiver
  (`trait_method_call`), a bounded parameter, `self` in a default body, and the trait named
  in the call. An implementation's type parameters match the trait's by position, renamed
  before the parameter types are compared, with the same bounds and `view` (`DZ0533`), and the
  help now prints the trait's signature with its type parameters. `is_own_self` tells `self`
  in a default body, whose type is spelled as the trait, from a trait object.
- **Monomorphization.** `ImplMethodInstance`: one implementation's body at its own
  arguments and the method's, taken from the `impl`'s method or from the trait's default
  for that implementer (`impl_default`, factored out of `impl_defaults`). A call on a concrete
  receiver asks for one; an `impl`'s generic method and a generic default are left out of
  the per-`impl` bodies, as templates.
- **Codegen.** Each instance is a free function, `<impl function>__<arguments>`, declared
  and defined behind include guards in the module that needs it. `object_safe` and the vtable
  leave generic methods out, so such a trait can still be a `^Trait`.

Tests: `tests/run/trait_generic_methods`, two modules: the trait and one `impl` in a
library, a plain and a generic `impl` in the program, a default generic method overridden
and inherited, inferred and written type arguments, a bound, a qualified call, a result of
the method's own type, and a trait object calling the other method. It is in the
warning-clean list, on clang++ and g++. `tests/errors/generic_trait_method_*` (`DZ0589`, two
`DZ0533`), `typed/`'s `Recorder`, `typeerrors/`'s `ShowAny` (now `DZ0589`), and
`dz explain DZ0589`.

Not covered: a trait object of a generic `impl` is still T-0049's, and a qualified call to
another module's trait method, `Writer::write(&mut log, 1)`, tripped a visibility check that
`log.write(1)` did not, which predated this: fixed in r140, a trait's methods being as
visible as the trait, and now in the fixture.
