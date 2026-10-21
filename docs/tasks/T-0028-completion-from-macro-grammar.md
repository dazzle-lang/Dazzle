---
id: T-0028
title: Completion from a macro's grammar
milestone: M9
status: open
kind: deferral
area: editor
where: `dz_lsp/src/ide.rs`, `completion`
---

**Completion from a macro's grammar** (§17.4 item 5): inside an invocation the editor completes the captured code — a state's locals, `btn.` — but does not offer the pattern's own words (`state`) or say which part of the grammar the cursor is in
