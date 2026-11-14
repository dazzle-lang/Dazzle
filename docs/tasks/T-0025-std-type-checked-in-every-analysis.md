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

**Most of what a build spent on `std` is gone** (2026-11-14), release `dz`, `words`, debug:

- **its C++ is no longer emitted every build.** `std` is a module like any other to T-0197's
  reuse: its record names the shared folder its files were compiled from, which is what kept
  it emitted, and it is kept when its sources, the root manifest (which decides its `#if`s),
  its placement and the generic bodies are unchanged and the folder still holds its files.
  Emitting went from 20 ms to 6 ms.
- **its checked layer is not copied.** `dz build` analyses once, so it hands `std`'s layer
  to the analysis (`dz_driver::analyse_once`, `dz_check::check_with`) rather than keeping
  it for a next analysis and copying it out: the type check 16 ms to 12 ms.
- **its files are parsed, and their trees copied, on every core**: 10 ms to 5.5 ms.

Found on the way and fixed, both larger: every build asked the C++ compiler `--version` up to
three times, 15 ms each (now kept per compiler file in the per-user cache); and a stamp that
had to hash an input was never written again, since its text was unchanged, so every build
re-hashed the shared `std`'s objects' inputs, 20 ms (`native::refreshed`).

**What is left**: type check ~11 ms and placement ~7 ms, nearly all `std`. Placement over
`std` is still needed, since what a program's bodies place depends on what `std`'s callees do.
The type check would need `std`'s checked layer kept on disk between processes: the type
table and the HIR serialised, which they are not built for (`Rc`-shared types). That is a
design of its own; until then, the editor checks `std` once per thread, and a build once.

## Where the code is

| What | Where |
|---|---|
| the analysis pipeline | `crates/dz_driver/src/lib.rs`, `analyse_selecting` |
| `std`'s checked layer, reused per thread | `crates/dz_driver/src/lib.rs`, `check`; `crates/dz_check/src/lib.rs`, `PreludeLayer` |
| `std`'s parsed trees, per process, on every core | `crates/dz_driver/src/lib.rs`, `std_trees`, `on_every_core` |
| the layer handed over in a one-shot build | `crates/dz_driver/src/lib.rs`, `analyse_once`; `crates/dz_check/src/lib.rs`, `check_with` |
| ownership leaving `std` out | `crates/dz_flow/src/lib.rs`, `check_files` and `check_std` |
| `std`'s C++ kept between builds | `crates/dz/src/reuse.rs`, `select`, `kept_shared`; `crates/dz/src/shared.rs`, `Shared::kept` |
