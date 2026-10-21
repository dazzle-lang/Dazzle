---
id: T-0023
title: No incremental analysis
milestone: M9
status: open
kind: deferral
area: editor
where: `dz_lsp/src/lib.rs`, `analyse`
---

No incremental analysis: every settle re-runs the whole project. Fine at today's sizes; §17.2's per-module cache is the answer

**Why a game needs it** (from the readiness review, 2026-09-14):

**Incremental analysis** in the LSP, and `std` not re-checked on every keystroke (T-0025;
M9).
