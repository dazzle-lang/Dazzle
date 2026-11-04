---
id: T-0132
title: Sanitizer builds in CI: the run corpus under AddressSanitizer and UBSan
milestone: none
status: done
kind: feature
area: tooling
---

Part of the testing [D-0094](../decisions/D-0094-safe-code-has-no-undefined-behaviour.md)
calls for; it needs no decision.

**Where the code is**

| Piece | Where |
|---|---|
| `DZ_CXXFLAGS`, added last to the command for Dazzle's code and the link | `compile` in `crates/dz/src/build.rs`; `dz`'s help in `crates/dz/src/main.rs` |
| The two ignored tests and `sanitized_run` | `every_program_is_clean_under_sanitizers`, `headless_examples_are_clean_under_sanitizers` in `crates/dz/tests/run.rs` |
| The CI job | `sanitizers` in `.github/workflows/ci.yml` |
| How to run it by hand | `docs/testing.md`, "Checking under the sanitizers" |

**Done** (2026-11-03). `DZ_CXXFLAGS` in the environment adds flags to the C++ command for
Dazzle's own code and the link; vendored libraries do not get them. CI's `sanitizers` job,
on Linux with clang++, runs every `tests/run` program with `-fsanitize=address,undefined
-fno-sanitize-recover=all -g -O1`, with regions on and off, and `examples/ownership` and
`examples/stb`. A report, a wrong output or a failed run fails it. Leak checking is off,
since a leak is not undefined behaviour and a panicking program leaks by design.

**What it found on its first run.** The shim's `subslice` built a slice as `&c[0] + lo`,
binding a reference to the first element even when there was none. An empty `String`'s
`as_str()` reached it with a null pointer: undefined behaviour from safe code, in
`collections`, `formatting` and `moves`. It now takes the start without reading an
element (`first_of`). Every program is clean since.

**What it was.** M5 checked `tests/run/ownership` under AddressSanitizer by hand. Nothing
ran that check, and `dz` could not pass the C++ compiler an extra flag. The programs of
D-0094 belong in `tests/flow` and `tests/errors`, as programs that must be refused; this job
catches what the checker misses in programs that run.
