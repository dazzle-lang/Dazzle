---
id: T-0025
title: `std` is type-checked in every analysis
milestone: M9
status: open
kind: deferral
area: editor
where: `dz_driver`, `analyse_for` and `check`; `dz_flow::check_files`
---

**`std` is type-checked in every analysis.** Its trees are parsed once per process (`dz_driver::std_tree`), which took a debug-build analysis of `tests/run/hello` from ~75 ms to ~61 ms (release: ~22 ms before). What remains is checking its ~2 200 lines each time, ~40 ms of the debug figure: symbols, the type table and HIR are whole-program, so reusing `std`'s needs the per-module analysis cache §17.2 already asks of M9

**Since then.** Within one process, `std`'s checked layer is kept per thread and reused
(`dz_driver::check`, `dz_check::PreludeLayer`), so the editor checks it once. A `dz build` is
one process, and still checks it every time.

**Ownership is no longer run over `std`** (2026-11-14). `dz_flow::check_files` skips its
bodies, since they are the same in every program and a body is checked against signatures
alone; `dz_flow::check_std` checks them in the test suite instead, with `TRACK_MEMORY` off
and on (`crates/dz_driver/tests/std_ownership.rs`) and with every branch in place
(`crates/dz_flow/tests/flow.rs`, `std_is_ownership_clean`). A release `dz check` of a
benchmark program went from 58–63 ms to 40–46 ms, ownership from 22–26 ms to 5–9 ms.

**What is left** for a build, release `dz`, `tests/bench_programs/words`: type check ~14 ms,
placement ~6 ms, lex and parse ~9 ms, nearly all of it `std`. Placement over `std` is needed,
since its C++ is emitted every build. The type check would need `std`'s checked layer kept
on disk between processes: the type table and the HIR serialised, which they are not built
for (`Rc`-shared types).

## Where the code is

| What | Where |
|---|---|
| the analysis pipeline | `crates/dz_driver/src/lib.rs`, `analyse_selecting` |
| `std`'s checked layer, reused per thread | `crates/dz_driver/src/lib.rs`, `check`; `crates/dz_check/src/lib.rs`, `PreludeLayer` |
| `std`'s parsed trees, per process | `crates/dz_driver/src/lib.rs`, `std_tree` |
| ownership leaving `std` out | `crates/dz_flow/src/lib.rs`, `check_files` and `check_std` |
