---
id: T-0041
title: A union variant carrying a struct with a constructor cannot be built
milestone: M12
status: open
kind: deferral
area: checker
where: `dz_check/src/check.rs`, `union_variant_literal`
---

**A union variant carrying a struct with a constructor cannot be built**: the variant literal is the struct's literal, which a type with a constructor refuses (DZ0543), and there is no spelling for building the variant through the constructor. The union itself compiles and drops correctly (r47) — how to spell `.Sound(5)` is a question for the user
