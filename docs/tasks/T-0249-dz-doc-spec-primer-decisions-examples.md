---
id: T-0249
title: dz doc: the spec, the primer, the decisions and the examples inside dz, searchable offline
milestone: M20
status: open
kind: feature
area: llm-tooling
where: `crates/dz/src/main.rs` (commands; `cmd_explain` already ships every diagnostic's text inside `dz`); `docs/spec/`, `docs/decisions/`, `examples/`
---

Asked by the user on 2026-11-18: an agent writing Dazzle should need no network once `dz`
is installed, and find everything the website has from `dz` itself.

**What it would take.** The documents are built into the binary, as `dz explain`'s texts
already are, so what `dz` says always matches the `dz` that says it:

- `dz doc` lists the topics; `dz doc §6.4` prints one section; `dz doc ownership` a topic
  by name.
- `dz doc search "<words>"` ranks sections of the spec, the primer (T-0064), the decisions
  and the examples, and prints each hit with its heading and a few lines, so an agent can
  follow up with `dz doc §N.M`.
- `dz examples` lists the examples and `dz examples show <name> [file]` prints one, so an
  example is a template an agent can copy without cloning the repository.
- `dz api` (T-0061) and `dz query` (T-0063) cover the code; `dz doc` covers the prose.

Plain text by default, `--format=json` for a tool. Size matters little: the spec is a few
hundred KB, and can be compressed.

**Not in scope:** the website (T-0251), which should end up generated from the same
files rather than kept in step by hand.
