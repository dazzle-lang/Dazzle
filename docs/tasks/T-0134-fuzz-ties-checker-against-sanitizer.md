---
id: T-0134
title: Fuzz the ties checker against a sanitizer
milestone: none
status: open
kind: feature
area: tooling
---

Part of the testing [D-0094](../decisions/D-0094-safe-code-has-no-undefined-behaviour.md)
calls for; it needs no decision. After
[T-0132](T-0132-sanitizer-builds-in-ci-run-corpus.md), whose flag hook it uses.

**What it is.** A generator of small programs, built from what the ties check reasons
about: containers and their `push`, `clear` and `swap_remove`; `&` and `&mut` arguments;
views kept across calls; unions and their payloads; `Rc` and `Box`; view structs and their
drops. Each program goes through `dz check`. Every program it accepts is built with the
sanitizer flags and run.

- An accepted program the sanitizer flags is a hole in the checker.
- A refused program that runs clean under the sanitizer is a false positive. Their count
  measures how much freedom a rule costs ([T-0123](T-0123-call-may-reach-memory-it-kills.md)
  asks exactly that).

**Why it matters.** The ties check is hand-written and has no formal model. Rust's borrow
checker had years of formal work and still had soundness bugs found by testing. Each hole
in D-0094 took minutes to find by thinking about one boundary; a fuzzer finds the next ones
without anyone having to think of them.

**What it would take.** A generator (`tools/fuzz_ties.py`, or a Rust test behind a
feature), the sanitizer build, and a corpus of minimised findings, each one landing in
`tests/flow` or `tests/errors`. The feature matrix
(`crates/dz_driver/tests/feature_matrix/`) already enumerates constructs in contexts, and
may be the place to start. Tooling only (`docs/log/`).
