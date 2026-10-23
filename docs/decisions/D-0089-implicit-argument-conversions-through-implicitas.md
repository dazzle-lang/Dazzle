---
id: D-0089
title: Implicit argument conversions through ImplicitAs
status: decided
---

**A type converts to `T` where it is passed as an argument to a parameter of type `T`, when
it implements `std`'s `ImplicitAs<T>`, whose `convert(&self) -> T` the compiler calls.
`std` implements it for `String` and `Path` to `cstr`. Chosen by the user on 2026-10-23.**

Calling C was the first case: `rl::DrawText(name.as_cstr(), …)` for every `String`, and
`format!` needing a `let` first. The user rejected a rule special to `String` in the
compiler: a type should declare its own conversions, as C++'s implicit constructors do.

- **A trait (chosen) over an `#[implicit]` attribute.** Both were written out. The trait
  can be a bound, `fn f<S: ImplicitAs<cstr>>(s: &S)`, and with the orphan rule counting a
  trait's arguments (D-0087) it covers a conversion into a module's own type from one it
  did not write, which had been the attribute's one advantage.
- **The name.** Carbon, a C++ successor, has the same feature under the same name. Rust's
  `Into` was rejected: it is explicit (`.into()`) and takes `self` by value, consuming what
  it converts, while this borrows. `Converts` sounded like a conversion that owns the value.
- **Only arguments.** Not `let`, a field, a `return` or an operator's operand, so a
  borrowed result such as a `cstr` into a `String` cannot outlive the call it was made for.
  That is also why passing the `String` is safer than calling `as_cstr()` by hand.
- **Borrowed, once, and opt-in.** The argument is the receiver of a `&self` method, so it is
  not moved and a temporary lives until the call returns. Never two conversions in a row.
  A method or constructor never converts on its own: C++ made conversion the default and
  `explicit` the opt-out, and its guidelines spend a rule on undoing that.
- **Not to choose or infer.** An imported overload set is still chosen by exact types
  (§15.3), and a generic parameter is inferred from the argument's own type (§8.1). Where a
  type converts to several types, the parameter's type picks which.
- **No numeric conversions.** §5.1 stands. Neither side of one would be a program's own, so
  only `std` could declare one, and it declares none.

Spec: §7.2, §5.3, §14.4, §15.2, §15.3.
