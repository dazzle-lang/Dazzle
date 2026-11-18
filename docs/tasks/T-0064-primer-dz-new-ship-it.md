---
id: T-0064
title: A primer, and `dz new` to ship it
milestone: M20
status: open
kind: feature
area: llm-tooling
---

**`dz new` is built (2026-11-04, with T-0076):** a manifest, a `main.dz` with a function, a
call of it and one `#[test]`, and a `.gitignore` (`cmd_new` in `crates/dz/src/main.rs`,
tested by `crates/dz/tests/new.rs`). What is left is the primer, and `dz new` dropping it in
as `AGENTS.md` / `CLAUDE.md`.

**A primer, and `dz new` to ship it.** `docs/primer.md`, about 300 lines, grown from
spec §0.1, plus a table of false friends for Rust and C++ programmers: `^` versus `&`;
`!->` is on the signature, never the type; `move()` resets its source (18.33); `&` cannot
be stored in a field; files have no imports (`dazzle.toml` names them, §3.2); `union` is
tagged; a constructor is named after its struct. Every snippet in it is compiled by a
test so it cannot go stale. `dz new <name>` (listed in §17.3, not built) scaffolds a
`dazzle.toml`, a `main.dz`, one `#[test]`, and drops the primer in as `AGENTS.md` /
`CLAUDE.md`.

**Changed by the user on 2026-11-18, for 0.3 (M20): agent files only when asked.** A
project someone makes by hand must not get files written for an LLM. So:

- `dz new <name>` stays as it is: no primer, no agent files.
- `dz new --agent <name>` adds them: `AGENTS.md` and `CLAUDE.md` (the primer, or a short
  file pointing at `dz doc`, T-0249, so it never goes stale against the `dz` installed), and
  a skill for Claude Code, `.claude/skills/dazzle/SKILL.md`, saying which `dz` command
  answers which question (`dz doc`, `dz api`, `dz query`, `dz check --format=json`,
  `dz explain`, `dz fix`).
- `dz agent init` adds the same files to a project that already exists, and never
  overwrites one that is there.
- `dz new` refuses, or warns, when the target is inside the Dazzle compiler's own checkout:
  `CLAUDE.md` files load from the working directory's parents, so a project there would
  load the compiler's working notes (T-0250).

The false-friends list above predates r135: `move` is gone and values move by default
(D-0106), so it is rewritten from the spec as it is, not copied from here.
