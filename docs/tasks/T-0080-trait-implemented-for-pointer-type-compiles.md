---
id: T-0080
title: A trait implemented for a pointer type compiles to C++ that calls the pointee
milestone: M12
status: open
kind: bug
area: codegen
---

**What is wrong.** An `impl` whose type is a pointer is accepted by the checker, and a
generic function bounded by the trait accepts a pointer — but the call inside it is emitted
as a call on what the pointer points at, which has no such method, so the C++ does not
compile (**`DZ0803`**, a compiler bug by invariant 7):

```dz
trait Show {
    fn show(&self) -> i32
}

impl<T: Format> Show for ^T {
    fn show(&self) -> i32 { return 1 }
}

fn twice<T: Show>(x: T) -> i32 {
    return x.show() * 2          // emitted as `::dz::obj(x).show()`: no member `show` in `Person`
}

let q: ^Person = &p
twice(q)
```

Calling it directly, `q.show()`, is no better: `.` reaches through the pointer (§5.2), finds
no `show` on `Person`, and reports `DZ0520` beside an `unsafe` error — so the
implementation can only ever be reached through a bound, which is the path that breaks.

**Why it matters.** It is a program the checker passes and the C++ compiler rejects, which
must never happen. Nothing in `std` needs an impl on a pointer today — formatting a pointer
was the obvious candidate, and D-0082 does it in the formatting macros instead because of
this — so it is rare in practice, but a user writing `impl Hash for ^Node` to key a map by
identity would hit it.

**Why it is this way.** Method calls reach through pointers and references by peeling them
(`Ty::peeled`) before the receiver's type is looked up, both in the checker
(`trait_method_call`, `impls_for(peeled)`) and in codegen, which wraps a pointer receiver in
`::dz::obj(…)`. Nothing considers that the pointer type itself might be what implements the
trait.

**What it would take.** One of two, and the user's call since the first changes what a
valid program is:

1. **Refuse an `impl` for a pointer type** in the checker, with a diagnostic saying to wrap
   the pointer in a struct. A language rule (a §21 row): smallest, and closes the hole.
2. **Support it**: look the receiver up as the pointer type first, then as what it points
   at, in the checker and in `dz_cpp`'s method emission, and do not wrap such a receiver in
   `obj`. Compiler work, plus matched `typed/` and `typeerrors/` cases (invariant 8).
