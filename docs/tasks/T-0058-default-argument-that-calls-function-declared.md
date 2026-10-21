---
id: T-0058
title: A default argument that calls a function declared later in the module does not compile
milestone: M12
status: open
kind: deferral
area: codegen
where: `dz_cpp`, function declaration emission
---

**What goes wrong.** C++ reads a default argument **where the function is declared**, and
`dz_cpp` emits functions in **source order**. So a default that calls a function declared
later in the same module names something that does not exist yet:

```dz
fn greet(name: str = default_name()) { … }   // the default is read here
fn default_name() -> str { return "world" }  // declared after
```

Dazzle itself has no forward-declaration rule — a module's declarations are all visible to
each other — so the program is valid and only the *emission* fails. Found by
`tests/run/formatting`.

**Why it matters.** It is an ordering constraint the language does not have, leaking out of
the backend: the user has to reorder their file for a reason nothing in Dazzle explains, and
the error comes from g++ pointing at generated code — which invariant 7 says is a compiler
bug, never a user error. It is also the kind of thing that appears as a project grows and
a file is reorganised, so the failure is detached in time from whatever caused it.

**The partial workaround, and its limit.** **Free functions could be declared twice** —
without defaults first, then with them, which C++ allows. That fixes free functions and
nothing else: **a method's defaults cannot be added in a later declaration**, so a method
whose default calls a later-declared function has no escape.

**What it would take.** The general fix is the same one two other rows want: **stop relying
on C++ defaults at all**. `dz_cpp` already fills defaulted arguments in at the call site
where it rewrites the call; if it did so everywhere and emitted no C++ default, the ordering
question would not arise, because there would be nothing in the declaration to read.

That single change closes three rows:

- this one — no default in the declaration, so no ordering constraint;
- [T-0039](T-0039-trait-method-non-constant-default-argument.md) — a trait method's
  non-constant default, which fails because the handle is emitted before the structs;
- [T-0047](T-0047-intrinsic-default-uses-c-compiler-builtins.md) — `#location` defaults
  whose column is 0 and whose function name is the C++ one, because they go through the
  shim's `caller_location` and its `__builtin_*` defaults.

Three unrelated-looking defects, one cause. That makes it the best-value piece of codegen
work on the M12 list: a `tests/run` case per row, and the three deferrals close together.
