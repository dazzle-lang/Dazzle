---
id: T-0033
title: Hygiene renames by name within one expansion
milestone: M10
status: open
kind: deferral
area: macros
where: `dz_macro/src/hygiene.rs`
---

**Hygiene renames by name within one expansion**, not by scope: a macro that writes one identifier as both a local and a module-level name sees both treated as the local
