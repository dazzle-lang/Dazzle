---
id: T-0211
title: Leaks are checked in no automatic test but the tracked run: check every program
milestone: none
status: done
kind: feature
area: tooling
---

Found 2026-11-13 with T-0208. A drop skipped wrongly (T-0118) is a leak and nothing else, so a
test that does not look for leaks cannot see one. `List::append` from a list lent elsewhere
skipped the drop of a list holding heap strings, a leak in safe code, and no test failed.

**Done 2026-11-13**, with the memory tracker rather than LeakSanitizer.
`every_program_frees_everything_it_allocates` builds every `tests/run` program that does not
panic with `--track-memory`, on every core, and fails on any leak report or any difference
from `expected.txt`. The tracker sees every allocation `Allocators::Global` makes, which is
where a wrongly skipped drop leaks; it runs on every platform the suite does, so it could be
checked here, where the sanitizer job runs only on Linux. It needed T-0210 first, so that a
static owning memory is dropped before the tracker reports. Every program passed when it was
turned on.

**Where leaks are looked for now.**

| Test | Leaks |
|---|---|
| `every_program_frees_everything_it_allocates` | every `tests/run` program that does not panic, tracked |
| `a_tracked_build_reports_leaks_and_bad_frees` | that the tracker reports a leak, a double free and a free of the wrong size |
| The sanitizer job, `every_program_is_clean_under_sanitizers` | not at all: `ASAN_OPTIONS=detect_leaks=0`. Turning LeakSanitizer on would also see C's own allocations, in the interop programs; it needs trying on the job, which no Windows machine can do |
| `tools/fuzz_ties.py` | not at all |

**Where the code is.**

| Piece | Where |
|---|---|
| The tracked runs | `every_program_frees_everything_it_allocates`, `a_tracked_build_reports_leaks_and_bad_frees` in `crates/dz/tests/run.rs` |
| `sanitized_run`, which sets `detect_leaks=0` | the same file |
| The tracker | `crates/dz_project/src/std/memory/tracker.dz` |
