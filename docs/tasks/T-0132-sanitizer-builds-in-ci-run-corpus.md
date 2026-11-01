---
id: T-0132
title: Sanitizer builds in CI: the run corpus under AddressSanitizer and UBSan
milestone: none
status: open
kind: feature
area: tooling
---

Part of the testing [D-0094](../decisions/D-0094-safe-code-has-no-undefined-behaviour.md)
calls for; it needs no decision.

**Where the code is**

| Piece | Where |
|---|---|
| The C++ command, built from fixed flags | `compile` in `crates/dz/src/build.rs` |
| CI and its compiler matrix (`DZ_CXX`) | `.github/workflows/ci.yml` |
| The manual recipe this automates | `docs/testing.md`, "Checking ownership under a sanitizer" |
| How `tests/run` is driven | `crates/dz/tests/run.rs` |

**What it is.** M5 checked `tests/run/ownership` under AddressSanitizer by hand, and
`docs/testing.md` says how. Nothing runs that check now, and `dz` cannot pass the C++
compiler an extra flag.

**Why it matters.** A checker's promises are only as good as the evidence for them. A
sanitizer run over every program that runs is the cheapest evidence there is. Regions
especially need it, since a wrong placement is a use after free that may print the right
answer.

**What it would take.**

- A way to pass flags for Dazzle's own code: `DZ_CXXFLAGS` in the environment, or
  `[build] cxxflags`.
- A Linux CI job with clang++. It builds every `tests/run` program, and the examples that
  run headless, with `-fsanitize=address,undefined -fno-sanitize-recover=all -g -O1`, once
  with regions off and once on (`DZ_REGIONS=1`).
- The job catches what the checker misses in programs that run. The programs of D-0094
  belong in `tests/flow` and `tests/errors`, as programs that must be refused.

Tooling only (`docs/log/`).
