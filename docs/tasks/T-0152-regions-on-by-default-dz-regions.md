---
id: T-0152
title: Regions on by default: DZ_REGIONS=0 is the heap-only build
milestone: none
status: done
kind: feature
area: codegen
---

T-0117's gate decided on 2026-11-02 that compiler-placed regions stay (spec §14.6), yet
codegen still placed values in regions only when `DZ_REGIONS` was set, so an ordinary `dz
build` or `dz run` put everything on the heap. This turns placement on in every build that
emits C++, and keeps the heap-only build one environment variable away, for benchmarks and
for telling a region bug from any other.

## Where the code is

| What | Where |
|---|---|
| The default: `Options::regions`, `true` in `Options::default` | `crates/dz_cpp/src/lib.rs` |
| The off switch: `DZ_REGIONS=0` (any other value, or none, leaves regions on), for every command that emits C++ | `emit_cpp`, `crates/dz/src/build.rs` |
| The editor's placement hint tooltip, which names the switch | `InlayHint` for `ide::HintKind::Placement`, `crates/dz_lsp/src/lib.rs` |
| Every run program on the default build (`every_program_produces_its_expected_output`, which clears `DZ_REGIONS`); each again on the heap, warning-clean on clang++ and g++ (`regions_change_nothing`); the default build's C++ warning-clean on every compiler present (`the_generated_code_builds_clean_under_wall_wextra`, `check_warning_clean`) | `crates/dz/tests/run.rs` |
| The A/B: heap against regions, `DZ_REGIONS` set to `0` or `1` either way, so an older `dz` (`--hand-dz`) still builds its heap side | `build`, `tools/bench_regions.py` |
| Dazzle against C++: the Dazzle half is the default build | `tools/bench_programs.py` |

## Built

**2026-11-03.** Compiler work; no language change: §14.6 already said where values go,
and only the note on what was not yet implemented changed. The switch is spelled as before
(`DZ_REGIONS`) with its meaning flipped: `0` was already "off", so a script that set `0`
or `1` explicitly means the same thing it did; one that relied on the variable being
*unset* for a heap build now gets regions.
