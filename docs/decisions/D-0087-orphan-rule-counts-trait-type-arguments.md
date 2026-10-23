---
id: D-0087
title: The orphan rule counts a trait's type arguments
status: decided
---

**An `impl` may be written with its trait, or where any of the types it joins is declared:
its type, then each of the trait's arguments in order, stopping at a bare type parameter.
Chosen by the user on 2026-10-23.** This widens D-0031, which said "with the trait or with
the type".

D-0031 was decided before traits took type arguments (`Mul<f32>` came with §18.27), and the
checker compared only the trait's module and the type's. So the module declaring `Vec2` could
not write `impl ImplicitAs<Vec2> for rl::Vector2`, a conversion *into* its own type, nor
`impl Mul<Vec2> for f32`, though both name its type. The user pointed out that the argument
is part of the trait being implemented, and asked how Rust treats it.

- **Rust's rule (chosen).** Since Rust 1.41 (RFC 2451), `impl ForeignTrait<LocalType> for
  ForeignType` is allowed. It stays coherent: two modules can only both write one
  implementation if each names the other's type, which would make their dependencies a
  cycle, and §3.2 makes a cycle an error (`DZ0006`).
- **The uncovered-parameter limit, also Rust's.** A bare type parameter before the local
  type ends the search: `impl<T> Pair<T, Mine> for Theirs` covers `Pair<Yours, Mine>`, which
  the module declaring `Yours` could also write without a cycle.
- **The old rule (rejected).** It refused implementations that cannot collide, and with it
  `ImplicitAs` (D-0089) could only convert *from* a module's types, never into them.

Spec: §7.6.
