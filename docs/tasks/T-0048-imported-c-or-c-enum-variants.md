---
id: T-0048
title: An imported C or C++ enum's variants are not in the type table
milestone: M12
status: open
kind: deferral
area: codegen
where: `dz_cpp/src/reflect.rs`, `describe_named`
---

**An imported C or C++ enum's variants are not in the type table**: its constants are the header's to spell, and `offsetof`-style reading of their values was not needed by anything yet. Its size, backing type and name are
