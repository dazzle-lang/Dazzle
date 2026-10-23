---
id: D-0088
title: The module carrying a recipe owns the header's types
status: decided
---

**For the orphan rule, an imported header's types are declared by the module whose
manifest carries the header's recipe. That module may implement any trait for them; it may
not add methods. Chosen by the user on 2026-10-23.**

It came from raylib's `Matrix`, which stores the same matrix as `std`'s `Mat4` transposed
(T-0046), so a cast between them is wrong and a conversion copies. The answer the user
chose: a raylib project uses `rl::Matrix` as its matrix type, and makes it pleasant. But an
imported header is a module of its own, so under D-0031 no Dazzle module could give
`rl::Matrix` a `*` (`std`'s `Mul`) or `rl::Vector2` a `+`.

- **Traits only (chosen).** The module carrying the recipe writes `impl Mul for rl::Matrix`
  as if it had declared `Matrix`. Two recipes for one library are an error (§15.1), so in any
  build exactly one module has the right, and two libraries can never both implement
  `Mul` for it. A wrapper package that carries the recipe for its users is that module; so
  is a project that imports raylib itself.
- **Traits and methods (rejected for now).** `impl rl::Matrix { pub fn identity() … }` would
  need a kind of `impl` block Dazzle does not have, since a struct's methods live in its
  body (§7.3). A trait the module declares itself already gives a header's type methods.
- **Extension methods from anywhere, scoped by import (rejected)**, as C# and Kotlin do:
  two libraries could each add a `length()` to one type, which is what D-0031 rules out.

Spec: §7.6, §15.2.
