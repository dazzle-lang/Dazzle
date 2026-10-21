---
id: D-0031
title: How is a trait implemented, and where?
status: decided
---

**Only by `impl Trait for Type { … }`, written with the trait or with the type** — Rust's orphan rule. A module can then add behaviour to a type it did not write (an editor implementing `Inspect` for an engine's structs, or for `f32`), and two libraries can never both implement a third's trait for a fourth's type. A struct no longer lists traits after `:`, which is reserved for inheritance (18.32). `Drop` and `Copy` become prelude traits, stricter than others: the struct's own module, every instance, no bounds on `Drop`, never called by name — they are its C++ destructor and copy constructor. A type's own method wins over a trait's; two traits providing one name are chosen between as `Trait::method(&v)`. An implementation for every type at once (`impl<T> X for T`) is not allowed (§6.1, §7.6)
