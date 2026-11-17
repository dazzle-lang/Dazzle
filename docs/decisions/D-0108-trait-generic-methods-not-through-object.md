---
id: D-0108
title: A trait's method may be generic, and is not called through a trait object
status: decided
---

**A trait's method may have type parameters of its own, `fn write<T: Format>(&mut self,
value: T)`. Each implementation has one function per type argument, made where a call's
receiver type is known: a concrete value, a parameter bounded by the trait, `self` in the
trait's own default bodies, or the trait named in the call, `Writer::write(&mut log, 3)`.
Through a trait object, `^Writer` or `&Writer`, such a method cannot be called (`DZ0589`);
the trait's other methods can. Chosen by the user on 2026-11-17, in T-0007 ("a vtable would
be bad for performance, probably option 1 is better"); spec §7.6, §8.1, r139. Amends D-0066,
which refused generic methods in a trait.**

**Before.** D-0066 refused a type parameter on any trait method (`DZ0557`), since a trait
object would have to call the method without knowing its arguments. The way around it was a
generic trait, `trait Writer<T>`, one implementation per type, or a trait reference as the
parameter, `fn write(&mut self, value: &Format)`, one indirect call per argument.

**The other option, not taken.** Every method stays callable through a trait object if each
object's vtable has a slot per instantiation the program uses. Then a vtable's layout depends
on calls anywhere in the program: a module's vtable changes when another module calls
`write<Foo>`. That works against each module's C++ being cached and rebuilt alone (invariant
3, T-0197), and the dispatch would be slower than a direct call for no gain over a bound.

**Why this one.** It is Rust's rule for the same case, so it is already familiar. It costs
nothing where it is not used: a generic method is a direct call, monomorphized like a generic
function (§8.4), and a trait object of the same trait keeps its vtable for the other
methods. The cost is that what a trait offers depends on how it is held: a generic method is
there on a value or a bound, not on a `^Trait`. That is said where it happens, at the call.

**What follows.**

- An implementation's method has the trait's type parameters by position, whatever it calls
  them, each bounded as the trait's is and `view` where it is (`DZ0533` otherwise).
- A trait with generic methods may still be held as a `^Trait`; its vtable has the other
  methods. A `virtual` method still may not be generic (`DZ0557`): a `virtual` method exists
  to be dispatched, so it has no static case to allow.
- Codegen: each implementation's instance is a free function named after the
  implementation's function and the type arguments, `Writer__app_Log__write__i32`, made in
  the module that needs it, behind include guards, like a generic method's (§8.4).
