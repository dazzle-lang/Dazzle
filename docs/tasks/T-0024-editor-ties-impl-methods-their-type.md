---
id: T-0024
title: Editor ties `impl` methods to their type by name
milestone: M9
status: open
kind: deferral
area: editor
where: `dz_lsp/src/ide.rs`, `methods_of`
---

`impl` methods and inherited trait defaults are tied to their type by name, so two same-named types in different modules could cross. The checker knows each implementation's real target; it should say
