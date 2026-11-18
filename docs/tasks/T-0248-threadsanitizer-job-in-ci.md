---
id: T-0248
title: A ThreadSanitizer job in CI
milestone: M19
status: open
kind: feature
area: tooling
where: `.github/workflows/ci.yml` (the `sanitizers` job, AddressSanitizer and UBSan, is the model)
---

The test of M19's promise. D-0096 promises safe code has no data race; the `sanitizers`
job (T-0132) checks the one-thread promise under ASan and UBSan, and this checks the
other one: every program in `tests/run` that uses threads, and `ecs` in parallel, built with
`-fsanitize=thread` on clang++ and g++ and run clean.

ThreadSanitizer cannot be combined with AddressSanitizer, so it is a job of its own. It
needs the memory tracker atomic first (T-0038), or `--track-memory` runs report races that
are real.
