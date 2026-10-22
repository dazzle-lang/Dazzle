---
id: T-0062
title: `dz explain <code>`
milestone: none
status: done
done: r72, 2026-10-22
kind: feature
area: llm-tooling
---

**`dz explain <code>`**, as `rustc --explain`: what the rule is, the spec section,
one wrong program and one corrected one. `tests/errors/` already holds one program per
mistake and can supply or check the examples, so the text cannot drift from the compiler.
