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

**Written 2026-11-27, not yet run.** The `tsan` job in `.github/workflows/ci.yml` runs
`threaded_programs_are_clean_under_thread_sanitizer` (`crates/dz/tests/run.rs`) once with
clang++ and once with g++: every `tests/run` program whose sources use `parallel`, `spawn`, a
`Mutex` or an atomic (`starts_threads`), with regions on, off and `--track-memory`, each
compared with its `expected.txt`, and `examples/ecs`; any ThreadSanitizer report fails it.
It sets `vm.mmap_rnd_bits=28` first, for the runner kernel's address randomisation, which
ThreadSanitizer's shadow memory does not fit. Neither this machine (Windows, where neither
compiler has ThreadSanitizer) nor its WSL Ubuntu (no compiler or Rust installed) has run it,
so it is done once it has run green, and any report it finds is fixed. Until then the job is
`continue-on-error`, so a first failure does not turn `master` red: take that off when it
passes. To run it here instead, WSL Ubuntu needs clang, g++, libclang-dev and Rust installed.

## Where the code is

| Piece | Location |
|---|---|
| The CI job | `.github/workflows/ci.yml`: `tsan` |
| The test, and what it selects | `crates/dz/tests/run.rs`: `threaded_programs_are_clean_under_thread_sanitizer`, `starts_threads`, `thread_sanitized_run` |
| The ASan and UBSan model | `crates/dz/tests/run.rs`: `every_program_is_clean_under_sanitizers`, `sanitized_run`; the `sanitizers` job |
