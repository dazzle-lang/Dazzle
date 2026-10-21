---
id: T-0046
title: Converting `Mat4` to raylib's `Matrix` copies field by field
milestone: M12
status: open
kind: deferral
area: stdlib
where: `dz_project/src/std/math/mat4.dz`
---

**`Mat4` and raylib's `Matrix`** hold the same numbers, but raylib's struct declares its fields row by row (`m0, m4, m8, m12, …`), so converting is a field-by-field copy rather than a cast
