---
id: T-0045
title: Methods on an untyped literal
milestone: M12
status: open
kind: deferral
area: checker
where: `dz_check/src/check.rs`, `call`
---

**Methods on an untyped literal**: `2.0.sqrt()` has no type to find `Float` on until the literal meets a context; write `(2.0 as f32).sqrt()` or bind it first
