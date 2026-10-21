---
id: T-0013
title: Hover on an imported name
milestone: M7a
status: open
kind: deferral
area: editor
where: `dz_lsp/src/ide.rs`
---

**Hover on an imported name** shows nothing: the editor queries build their text from Dazzle declarations. Go-to-definition should already land in the header, since the symbol's span points there — untested
