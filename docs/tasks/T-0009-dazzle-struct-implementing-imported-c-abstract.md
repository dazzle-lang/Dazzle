---
id: T-0009
title: A Dazzle struct implementing an imported C++ abstract class
milestone: M7b
status: open
kind: feature
area: interop
---

**What is missing.** Subclassing an imported C++ class from Dazzle, and having C++ call back
into the Dazzle methods through its vtable:

```dz
struct MyListener : jolt::ContactListener {
    fn OnContactAdded(&mut self, a: ^jolt::Body, b: ^jolt::Body) override { … }
}

physics.SetContactListener(&my_listener)      // C++ dispatches into Dazzle
```

Today naming an imported class as a base is refused outright
([T-0018](T-0018-inheritance-between-imported-c-classes.md), `DZ0541`), so this does not
start.

**Why it matters.** This is **how modern C++ libraries are extended**, and the libraries
that matter for a game are extended this way or not at all. Jolt's `ContactListener`,
`BroadPhaseLayerInterface` and `ObjectLayerPairFilter` are abstract classes the user is
*required* to subclass — there is no function-pointer alternative — so a Dazzle game cannot
use Jolt at all without this. ImGui's rendering and platform backends are the same shape.
A C library's callback is a function pointer and already works; a C++ library's callback is
usually a virtual method, and that is this row.

**What it would take, in order.**

1. [T-0018](T-0018-inheritance-between-imported-c-classes.md): the importer records the base
   relation, `collect_base` accepts an imported class as a base, and `&Derived` → `&Base`
   emits a C++ conversion. That alone gets the *layout* right.
2. **Virtual dispatch in the other direction.** Dazzle's `override` currently overrides a
   Dazzle `virtual`. Overriding an imported `virtual` means the generated C++ struct
   genuinely deriving from the header's class and writing `override` on the method, so the
   C++ compiler builds the vtable — which is the easy part, since Dazzle emits C++ and C++
   already knows how to do this. The work is in the checker: matching a Dazzle method
   against an imported virtual's signature, with the pointer and reference conventions of
   §15.2 in between, and reporting a mismatch at the method rather than letting g++ report
   it in generated code (invariant 5).
3. **What a pure virtual means.** An abstract class has methods with no body, and an
   implementer must provide all of them. That is what a Dazzle `trait` says, so the natural
   mapping is "an imported abstract class is checked like a trait's required methods", and
   the diagnostic for a missing one should read like the trait one does.
4. **Lifetime and ownership at the boundary.** `SetContactListener(&my_listener)` hands C++
   a pointer it will hold across frames, and a `&T` cannot be stored in a Dazzle field
   (D-0003) precisely because Dazzle does not track that. Here the C++ side stores it. §1.1
   says a dangling `^T` is the programmer's problem, so the answer is presumably that this
   takes a `^mut T` and the programmer keeps the listener alive — but it should be said
   deliberately rather than fallen into.
5. [T-0010](T-0010-proof-on-real-libraries-dear-imgui.md) is how it gets proven: a Jolt
   fixture with a real `ContactListener` is the test that this works, and nothing smaller
   really is.

Steps 2–4 each want a decision recorded; step 1 is prerequisite work with no design in it.
