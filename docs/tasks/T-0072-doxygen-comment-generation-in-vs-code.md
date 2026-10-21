---
id: T-0072
title: Doxygen comment generation in the VS Code extension
milestone: M9
status: open
kind: feature
area: editor
---

**Doxygen comment generation in the VS Code extension** — typing `///` above a
declaration expands to a stub with one `@param` per parameter and an `@return` when the
signature has one. Entirely extension-side: the client already runs `dz lsp`, and the
outline query already knows every declaration's parameters, so the information is there.
Self-contained, and the right size for a first contribution to `editors/vscode`.
