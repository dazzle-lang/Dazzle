---
id: T-0055
title: A pointer has no `Format`
milestone: M12
status: done
done: r69, 2026-10-22
kind: deferral
area: stdlib
where: `dz_project/src/std/core/format.dz`
---

**What is refused.** No formatting macro accepts a pointer:

```dz
println!("{}", p)       // DZ0527: `^mut Enemy` does not implement `Format`
```

The diagnostic says what to do instead — write what it points at (`p^`), or, for the
address, cast it: `p as usize` with `{:x}`.

**Why.** Writing a pointer means one implementation covering every pointee type:
`impl<T> Format for ^T`. That is a **generic `impl`**, and a trait object of a type whose
implementation is generic has no vtable — one implementation written, an unbounded number of
instances, each needing its own
([T-0049](T-0049-trait-objects-that-are-not-object.md)). Since a formatting macro's
arguments go through `&Format`, an indirect call each
([T-0056](T-0056-formatting-macro-arguments-go-through-format.md)), there is no vtable to
call and the argument cannot be accepted.

Until r64 `println!` did print a pointer's address, through a path that did not need the
vtable; that was removed when formatting moved to `&Format`.

**Why it matters.** Printing an address is debugging, not output, and it is the one thing
a `{:p}`-shaped spec exists for in other languages: checking whether two handles refer to
the same thing, or that a pointer is where you expected. `p as usize` with `{:x}` gives
exactly that in one more token, so the cost is small — but the *error* is confusing, because
"does not implement `Format`" sounds like an oversight in `std` rather than a consequence of
how trait objects work.

**What it would take.** Three ways out, cheapest first:

1. **A `{:p}` spec**, handled by the macro itself rather than by a `Format` impl: the checker
   accepts a pointer argument only under `{:p}` and lowers it to the `usize` cast it already
   tells people to write. No vtable, no `impl`, and the spelling says "address" rather than
   looking like an accident. This is the one to build, and it fits naturally into
   [T-0005](T-0005-format-specifiers.md)'s spec-set work.
2. Make the formatting path **statically dispatched** per placeholder, which is what §12.9
   originally described and what [T-0056](T-0056-formatting-macro-arguments-go-through-format.md)
   is about — then a generic `impl` needs no vtable and this falls out for free.
3. Lift the generic-`impl` trait-object restriction, which is the large
   [T-0049](T-0049-trait-objects-that-are-not-object.md) work.
