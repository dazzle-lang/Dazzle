---
id: T-0211
title: Leaks are checked in no automatic test but the tracked run: turn on LeakSanitizer
milestone: none
status: open
kind: feature
area: tooling
---

Found 2026-11-13 with T-0208. A drop skipped wrongly (T-0118) is a leak and nothing else, so a
test that does not look for leaks cannot see one. `List::append` from a list lent elsewhere
skipped the drop of a list holding heap strings, a leak in safe code, and no test failed.

**Where leaks are looked for today.**

| Test | Leaks |
|---|---|
| The sanitizer job, `every_program_is_clean_under_sanitizers` | not at all: `ASAN_OPTIONS=detect_leaks=0`, since "a leak is not undefined behaviour, and a program that panics leaks by design" |
| `a_tracked_build_reports_leaks_and_bad_frees` | only for the programs it names. Until T-0208 it compared stdout alone, and the report goes to stderr with the program still succeeding, so it caught no leak at all; it now fails on a report, and names the region programs too |
| `tools/fuzz_ties.py` | not at all |

**What it would take.** In the sanitizer job, `detect_leaks=1` for the programs whose
`expected.txt` does not start with `panic!` (LeakSanitizer runs on Linux, where the job does).
Memory a static still holds at exit is reachable, so it should not be reported, but that needs
checking on the job itself, which no Windows machine can. Every program in `tests/run` could
also run tracked, once T-0210 lets a static own memory.

**Where the code is.**

| Piece | Where |
|---|---|
| `sanitized_run`, which sets `detect_leaks=0` | `crates/dz/tests/run.rs` |
| The tracked runs | `a_tracked_build_reports_leaks_and_bad_frees` in the same file |
| The CI job | `sanitizers` in `.github/workflows/ci.yml` |
