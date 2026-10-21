---
id: T-0021
title: An untyped imported integer is not range-checked
milestone: M7b
status: open
kind: deferral
area: checker
where: `dz_check/src/lib.rs`, `collect_foreign`
---

An untyped imported integer is not range-checked against its context: `let b: u8 = rl::SOME_BIG` reaches C++ as a narrowing `static_cast`. The importer knows the value; the checker should compare it, as it does a literal's
