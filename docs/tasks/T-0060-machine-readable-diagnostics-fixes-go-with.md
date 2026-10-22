---
id: T-0060
title: Machine-readable diagnostics, and fixes to go with them
milestone: none
status: done
done: r72, 2026-10-22
kind: feature
area: llm-tooling
---

**Machine-readable diagnostics, and fixes to go with them.** `dz_diag` already has
`Fix`/`Edit`, but at 2026-10-20 only 2 of the 143 codes used it; 118 diagnostics carry a
prose `help:` instead. Three parts: `dz check --format=json` (code, severity, spans,
message, help, fix edits as byte ranges); `dz fix` to apply the fixes; and turning the
mechanical helps into real fixes — insert `move()`, add the missing `switch` arms, add
`impl Drop`, the "did you mean" renames. Exact edits beat prose for a model.
