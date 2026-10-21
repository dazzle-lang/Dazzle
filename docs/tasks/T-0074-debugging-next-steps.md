---
id: T-0074
title: Debugging, next steps
milestone: M9
status: open
kind: feature
area: editor
---

**Debugging, next steps** (step one, `dz build` handed to a native debugger with `dazzle.natvis`, is done): gdb pretty-printers to match the natvis; showing a
monomorphized `List__i32` as its elements (natvis cannot match a name pattern that is not
a template, so this needs either a common template base in the shim or generated natvis);
and keeping `_dz_*` temporaries out of the locals list, which would mean scoping them in
blocks of their own.
