---
id: T-0052
title: Test a trait default method that builds a `Self`
milestone: M12
status: open
kind: deferral
area: generics
where: `dz_mono`, `inherited_defaults`
---

A trait default method is exercised by `tests/run/generics` only for bodies that call the trait's own methods; one that builds or returns a `Self` value has no test
