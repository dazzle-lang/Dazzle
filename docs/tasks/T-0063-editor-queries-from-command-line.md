---
id: T-0063
title: Editor queries from the command line
milestone: none
status: open
kind: feature
area: llm-tooling
---

**Editor queries from the command line.** `dz query hover <file>:<line>:<col>`,
`dz query definition …`, `dz query members <Type>`. `dz_lsp/src/ide.rs` already answers
these from an `Analysis` and an offset with no protocol, so the CLI is a thin wrapper —
and an agent without an LSP client can ask "what type is this, what can I call on it".
