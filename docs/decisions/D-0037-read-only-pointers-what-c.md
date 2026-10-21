---
id: D-0037
title: Read-only pointers, and what C's `const T*` becomes
status: decided
---

**`^T` is read-only and `^mut T` writable**, standing to each other as `&T`/`&mut T` and `[]T`/`[]mut T` do: read-only unless `mut` is written. `const T*` maps to `^T` and `T*` to `^mut T` (§15.2). `&T` → `^T` and `&mut T` → `^mut T` are implicit, anything writable serves where the read-only form is wanted, and nothing gains writability; `or` gives a `&T` from a `^T` and a `&mut T` from a `^mut T`; `.ptr` is as writable as its view; the owning pointer is `own ^mut T`. Behind a `^mut` the pointee must match exactly, for the reason C forbids `T**` → `const T**`. A `^const T` was rejected as the Rust shape, which reads against the rest of the language. Forced by M7: with one writable pointer, every read-only C parameter needed an `&mut` at the call (§5.2, §5.3)
