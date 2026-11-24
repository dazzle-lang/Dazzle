---
id: D-0115
title: A constructor is private unless it says otherwise
status: decided
---

**Decided by the user on 2026-11-23.** Until then the checker ignored a constructor's
visibility: one written `pub(module)` or `protected` could be called from any module, and §4
did not mention constructors at all. In the spec as §4, r148.

**A constructor is a member, and follows §4 as a field and a method do.**

| Written | Who may call it |
|---|---|
| nothing | the struct's own functions |
| `protected` | also the structs that inherit from it, including as `: Base(…)` |
| `pub(module)` | anywhere in the module |
| `pub` | importers too |

**The alternative was public by default**, which would have changed no program but made
constructors the one exception to "default is private", with no way to write a fully private
one. Every constructor in the repository that had no keyword was given `pub`, so none lost
access by this change.

**Default initialisation follows it**, the user's call the same day: "we should do like Rust
and C++, that they don't allow `let x: T` when `T` has a private constructor" (§6.2,
[T-0262](../tasks/T-0262-type-with-private-constructor-can-still.md)). A type with constructors
is default-built only through one that takes no arguments and is visible there. That reaches
arrays, tuples, a struct's fields and each instance of a generic, and a constructor must assign
a field whose type has no default. `Box`, `Rc` and `Weak` keep `null` as their default (§14.5).
