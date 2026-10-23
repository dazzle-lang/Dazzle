---
id: T-0088
title: An explicit As<T> trait: user-defined conversions written with as
milestone: M12
status: open
kind: feature
area: language
---

**What is missing.** `ImplicitAs<T>` (D-0089) gives a type a conversion the compiler makes on
its own, for arguments only. There is no way to give a type a conversion that is *written*:
`x as T` works for the numbers, `"x" as cstr`, `c as str` and pointers, all built into the
compiler (§10, §5.3), and nothing else. So a conversion that should never be silent, or one
wanted outside an argument (a `let`, a `return`, a field), has to be an ordinary method with a
name of its own: `tint.to_rl()`, `v.to_vec2()`, each spelled differently.

**Why it matters.** `as` is already how Dazzle says "convert this": readers look for it, and
it is the explicit half of what `ImplicitAs` is the implicit half of. Carbon, where the name
came from, pairs them the same way: `As(T)` is what `x as T` calls, and `ImplicitAs(T)`
extends it. One spelling for every conversion would also give the help in
`implicit_as_help` ("convert it yourself") something concrete to suggest: `name as cstr`.

**What it would take.** A design decision first:

- `pub trait As<T> { fn convert(&self) -> T }` in `std`, and `x as T` calling it when no
  built-in cast applies. Does `ImplicitAs<T>` require `As<T>` (Carbon's choice, so every
  implicit conversion can also be written), or are they independent?
- The orphan rule already counts a trait's arguments (D-0087), so `impl As<Vec2> for
  rl::Vector2` belongs with `Vec2` as `ImplicitAs` does.
- A built-in cast must keep winning: `n as i64` is never a trait call, and §5.1's numeric
  rules do not change.
- Unlike `ImplicitAs`, the result may be kept (`let c = name as cstr`), so a conversion that
  borrows needs the same view-of-a-temporary check `as_cstr()` has (`DZ0407`, §6.5).

Then: the checker's `as` (`check.rs`, the cast arm), §10's cast table, §7.2, §14.4, a §21
row, and a case each in `typed/` and `typeerrors/`.
