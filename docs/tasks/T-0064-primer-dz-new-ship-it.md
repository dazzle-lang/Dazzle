---
id: T-0064
title: A primer, and `dz new` to ship it
milestone: none
status: open
kind: feature
area: llm-tooling
---

**A primer, and `dz new` to ship it.** `docs/primer.md`, about 300 lines, grown from
spec §0.1, plus a table of false friends for Rust and C++ programmers: `^` versus `&`;
`!->` is on the signature, never the type; `move()` resets its source (18.33); `&` cannot
be stored in a field; files have no imports (`dazzle.toml` names them, §3.2); `union` is
tagged; a constructor is named after its struct. Every snippet in it is compiled by a
test so it cannot go stale. `dz new <name>` (listed in §17.3, not built) scaffolds a
`dazzle.toml`, a `main.dz`, one `#[test]`, and drops the primer in as `AGENTS.md` /
`CLAUDE.md`.
