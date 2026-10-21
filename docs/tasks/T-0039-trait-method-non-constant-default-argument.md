---
id: T-0039
title: A trait method's non-constant default argument
milestone: M12
status: open
kind: deferral
area: codegen
where: `dz_cpp`, `trait_object_decls`
---

**What goes wrong.** A trait method may have default arguments, and they are the **trait's**
rather than each implementer's (§7.6) — so a trait object's handle has to declare them, which
`trait_object_decls` does. But that handle is emitted **before the module's structs are
defined**, because it is what the structs' methods are declared in terms of. A default that
is a constant is fine there; one that names a type is not:

```dz
pub trait Spawner {
    fn spawn(&self, count: i32 = 1)          // fine
    fn spawn_at(&self, at: Vec3 = Vec3{})    // the default would not compile where it is written
}
```

`Vec3{}` appears in the generated header at a point where `Vec3` is not yet a complete type,
so g++ rejects it.

**Why it was accepted.** No trait in `std` or in the examples needs one, and the workaround
is to take the parameter without a default and overload the *call* with an explicit value.
Reordering the emitted header to put struct definitions before trait handles is not possible
in general — a trait object can appear in a struct's own fields, so the dependency goes both
ways.

**What it would take.** The fix is not to reorder but to **stop putting the default in the
C++ declaration at all**: codegen already fills defaulted arguments in at the call site where
it rewrites the call, and if it did so for every call, the C++ declaration would need no
default and the ordering problem would disappear. That is the same change
[T-0047](T-0047-intrinsic-default-uses-c-compiler-builtins.md) wants for a different reason
— it is the reliance on *C++* defaults that costs the column and the qualified function
name there — so the two are one piece of work:

> make `dz_cpp` supply every defaulted argument at the call site, and emit no C++ default
> anywhere.

Done once, it closes both rows, and it removes a whole class of "the default did not compile
where it landed" problems rather than this one instance. A test would put a `Vec3{}` default
on a trait method and call it through both a concrete type and a `^Trait`.

Related: [T-0058](T-0058-default-argument-that-calls-function-declared.md), which is the
same reliance on C++ defaults failing a third way — a default that calls a function declared
later in the module.
