---
id: T-0055
title: A pointer has no `Format`
milestone: M12
status: open
kind: deferral
area: stdlib
where: `dz_project/src/std/core/format.dz`
---

**A pointer has no `Format`**, so no formatting macro takes one — `println` used to print its address, until r64 — since a generic `impl<T> Format for ^T` has no vtable (the trait-object row above). Write what it points at, or cast it: `p as usize` with `{:x}`
