---
id: T-0134
title: Fuzz the ties checker against a sanitizer
milestone: none
status: done
kind: feature
area: tooling
---

Part of the testing [D-0094](../decisions/D-0094-safe-code-has-no-undefined-behaviour.md)
calls for; it needs no decision. After
[T-0132](T-0132-sanitizer-builds-in-ci-run-corpus.md), whose flag hook it uses.

**Where the code is**

| Piece | Where |
|---|---|
| The generator, the runs and the report | `tools/fuzz_ties.py` (its docstring says how to run it) |
| Building a refused program anyway | `allow_for_fuzzing` in `crates/dz/src/main.rs`: `DZ_FUZZ_ALLOW`, read by a debug `dz` only |
| CI | `ties_fuzzer_finds_no_hole_under_sanitizers` in `crates/dz/tests/run.rs`, run by the `sanitizers` job: seed 1, 200 cases, about 35 seconds |
| How to read its output | "Fuzzing the ties check" in `docs/testing.md` |

**Done** (2026-11-04). The generator takes views into lists of ints, long strings and short
ones (whose text is in the `String`, D-0095), a `String`, a hash map, a `Box`, two `Rc`s to
one list, a union's payload, a struct's two fields, a list of lists and a map of lists; by
`let`, a call returning a view, `if let`, `switch` and `for`; then kills through direct
calls, writes, helpers, two arguments and calls that also read a view; then uses. With
every value on the heap and every kill growing its container far enough to move it:

| Run | Accepted | of them flagged | Refused | of them flagged when run | ran clean |
|---|---:|---:|---:|---:|---:|
| seed 4, 1,000 cases, g++ | 605 | 0 | 395 | 240 | 148 |
| seed 5, 2,000 cases, clang++ | 1,284 | 0 | 716 | 439 | 267 |

No hole: every accepted program ran clean (or panicked, as `swap_remove` after `clear`
does). About 60% of refusals are confirmed by the sanitizers. The rest are mostly what they
cannot see: a view read after `clear()`, which keeps the array; a union's payload after its
variant changed; a long `String`'s text, which survives the `String` moving. What remains
is the cost of rules: two `&mut` arguments naming one kill when nothing is held across
the call (`grow_both(&mut xs, &mut xs)`), and an element of a list of lists kept across a
kill of another element (`&nest[0][1]` across a growth of `nest[1]`), which the check sees
as one `nest[*]`, as Rust does. No finding needed adding to `tests/flow`; the corpus of
minimised findings starts empty.

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
