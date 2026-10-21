---
id: T-0049
title: Trait objects that are not object-safe
milestone: M12
status: open
kind: deferral
area: codegen
where: `dz_cpp`, `object_safe`; `dz_check`, `refuse_generic_impl_object`
---

**Trait objects that are not object-safe** — of a generic trait, one with associated types, or one whose methods mention `Self` — report `DZ0801`, and so does a trait object of a type whose `impl` is generic, which would need a vtable per instance.

**Why a game needs it** (from the readiness review, 2026-09-14):

**Trait objects that are not object-safe today** — of a generic trait, or of a type
whose `impl` is generic.
