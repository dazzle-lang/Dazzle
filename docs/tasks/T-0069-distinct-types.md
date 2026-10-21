---
id: T-0069
title: Distinct types
milestone: none
status: open
kind: design
area: language
---

**design: distinct types**, e.g. `type Radians = distinct f32`, as in Odin. Catches
degrees against radians and pixels against world units at compile time; fits the rule
that no numeric conversion is implicit.
