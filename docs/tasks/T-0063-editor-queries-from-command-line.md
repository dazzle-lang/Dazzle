---
id: T-0063
title: Editor queries from the command line
milestone: M20
status: open
kind: feature
area: llm-tooling
---

**Editor queries from the command line.** `dz query hover <file>:<line>:<col>`,
`dz query definition …`, `dz query members <Type>`. `dz_lsp/src/ide.rs` already answers
these from an `Analysis` and an offset with no protocol, so the CLI is a thin wrapper —
and an agent without an LSP client can ask "what type is this, what can I call on it".

**Added for 0.3 (M20), 2026-11-18:** `dz query references <file>:<line>:<col>` and
`dz rename <file>:<line>:<col> <new>` (with `--dry-run`), over `ide.rs`'s `references` and
`rename`, which the LSP already serves. An agent has text search and replace of its own;
what `dz` adds is the semantic version, which finds a name by what it resolves to, not by
how it is spelled. Plain text by default, `--format=json` for a tool.
