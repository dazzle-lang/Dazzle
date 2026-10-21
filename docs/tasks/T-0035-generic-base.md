---
id: T-0035
title: A generic base
milestone: M12
status: open
kind: deferral
area: checker
where: `dz_check/src/lib.rs`, `collect_base`
---

**A generic base.** `struct Button : Panel<i32>` is refused (`DZ0541`): inheriting an instantiation means laying out one base per instance, which monomorphization does not do. A generic *derived* type over a concrete base works

**Why a game needs it** (from the readiness review, 2026-09-14):

**A generic base** — `struct Button : Panel<i32>` (`DZ0541`).
