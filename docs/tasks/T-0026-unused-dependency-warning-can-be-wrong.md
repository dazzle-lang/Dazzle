---
id: T-0026
title: An unused-dependency warning can be wrong for a dependency used only through `.`
milestone: M9
status: open
kind: deferral
area: resolve
where: `dz_resolve`, `report_unused_dependencies`
---

**An unused-dependency warning can be wrong for a dependency used only through `.`**: a trait method it adds, called on a value whose type comes from elsewhere, is resolved by the checker, which does not mark the dependency used. Name the trait, or ignore the warning
