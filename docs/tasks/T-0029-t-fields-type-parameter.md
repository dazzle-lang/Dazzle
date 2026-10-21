---
id: T-0029
title: `T::fields` of a type parameter
milestone: M10
status: open
kind: deferral
area: checker
where: `dz_check`, `path_type_query`
---

**`T::fields` of a type parameter** at compile time. Macros expand before types exist (18.74), so a per-field macro takes the declaration (§13.4); iterating a parameter's fields is a loop unrolled per instance, not a macro
