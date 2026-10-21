---
id: T-0025
title: `std` is type-checked in every analysis
milestone: M9
status: open
kind: deferral
area: editor
where: `dz_driver`, `analyse_for`
---

**`std` is type-checked in every analysis.** Its trees are parsed once per process (`dz_driver::std_tree`), which took a debug-build analysis of `tests/run/hello` from ~75 ms to ~61 ms (release: ~22 ms before). What remains is checking its ~2 200 lines each time, ~40 ms of the debug figure: symbols, the type table and HIR are whole-program, so reusing `std`'s needs the per-module analysis cache §17.2 already asks of M9
