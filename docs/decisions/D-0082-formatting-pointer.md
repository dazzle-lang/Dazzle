---
id: D-0082
title: Formatting a pointer
status: decided
---

**`{}` writes what a pointer points at, and `{:p}` writes its address; chosen by the user on 2026-10-22** ("if I try to format a `^Person`, I want to see the Person … to print the pointer, we could have `{:p}`"). This reverses the part of D-0080 that said a pointer has no `Format`, which was a consequence of formatting through `&Format` rather than a choice ([T-0055](../tasks/T-0055-pointer-has-no-format.md)).

- **A pointer is written as a reference is**, one level down: `println!("{}", p)` with `p: ^Person` writes the `Person` by its own `Format`, and a spec applies to what it points at, so `{:x}` of a `^i32` is the integer in hex. A pointer to a pointer is an error whose help says to write `p^`.
- **A null pointer is written `null`**, rather than read through. Claude's call, made while implementing: reading through it would crash the program that is trying to report something.
- **`{:p}` is the address**, in hex after `0x` on every platform (C's `%p` differs between libraries), right-aligned in a width as a number is. It suits a pointer only, and a zero fill is refused, since the zeros would land before the `0x`.
- **It is the formatting macros' doing, not an `impl Format for ^T`.** A trait implemented for a pointer type is not supported (the compiler accepts one and then emits C++ that calls the method on the pointee, [T-0080](../tasks/T-0080-trait-implemented-for-pointer-type-compiles.md)), so a `T: Format` bound is not satisfied by a pointer. The macros handle a pointer argument themselves, through `Formatter::write_pointee` and `write_address`.
- **`error(…)` writes a pointer only under `{:p}`**: its message is built without allocating and without `Format` (§9.2), so it has no way to write what a pointer points at, and `{}` on one is an error there (`DZ0527`).

What made it possible is [T-0056](../tasks/T-0056-formatting-macro-arguments-go-through-format.md), done in the same revision: each placeholder is written by a direct call to its argument's `Format::format`, so a generic method such as `write_pointee<T: Format>` needs no vtable (§12.9, r69).
