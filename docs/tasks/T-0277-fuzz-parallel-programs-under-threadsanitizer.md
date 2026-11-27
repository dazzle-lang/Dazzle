---
id: T-0277
title: Fuzz parallel programs under ThreadSanitizer
milestone: M19
status: open
kind: feature
area: testing
---

`tools/fuzz_ties.py` (T-0134) writes programs no one wrote by hand and runs those the checker
accepts under AddressSanitizer: one that is accepted and reaches freed memory is a hole in the
ties check. CI runs a fixed seed (`ties_fuzzer_finds_no_hole_under_sanitizers` in
`crates/dz/tests/run.rs`).

The same for threads: generate programs with `parallel` blocks and `parallel for` loops over
locals, fields, elements, parameters and `Rc`s, writing and reading them in every combination,
and run every one the checker accepts under ThreadSanitizer (`-fsanitize=thread`). An accepted
program with a report is a hole in §22's checks. A fixed seed in CI, beside T-0248's job, and
more seeds by hand.

Needs T-0248 running first, so a report can be told from a problem with the job.

## Where the code is

| Piece | Location |
|---|---|
| The single-threaded fuzzer | `tools/fuzz_ties.py`; `ties_fuzzer_finds_no_hole_under_sanitizers` in `crates/dz/tests/run.rs` |
| Running under ThreadSanitizer | `thread_sanitized_run` in `crates/dz/tests/run.rs` (T-0248) |
