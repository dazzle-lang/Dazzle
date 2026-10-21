---
id: T-0056
title: A formatting macro's arguments go through `&Format`
milestone: M12
status: open
kind: deferral
area: stdlib
where: `dz_check/src/check.rs`, `format_macro`; `std/core/formatter.dz`, `write_args`
---

**A formatting macro's arguments go through `&Format`**, one indirect call each, rather than the statically dispatched `Format::format` call per placeholder §12.9 once described. That was built first and dropped: as a sequence of statements it ended each argument's temporaries before the line was written, where a call keeps them (18.80). The direct form needs codegen to keep one statement's temporaries across several, or a `write_args` instantiated per argument list
