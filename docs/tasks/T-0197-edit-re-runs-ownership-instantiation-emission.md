---
id: T-0197
title: An edit re-runs ownership, instantiation and emission only for the modules it can affect
milestone: M15
status: done
kind: feature
area: tooling
where: `crates/dz/src/reuse.rs` (records, `select`, `selection`, `reconcile`, `write`); `emit_reusing` and `verify_whole` in `crates/dz/src/main.rs`; `analyse_selecting` and `Select` in `crates/dz_driver/src/lib.rs`; `check_files` in `crates/dz_flow/src/lib.rs`; `monomorphize_only` in `crates/dz_mono/src/lib.rs`; `emit_selected`, `Selection`, `place_instance_body` (`claimed`, `placed`) in `crates/dz_cpp/src/lib.rs`; tests `crates/dz/tests/reuse.rs` and the edit corpus `crates/dz/tests/incremental.rs`
---

**Done** (2026-11-11), stage 2a of the incremental-build plan. **What was wrong:** every
build checked every module's bodies for ownership, instantiated every module's generics
and wrote every module's C++. At 100 modules that was 1.2 s of an edit's 1.7 s of
Dazzle's own work, for modules the edit could not change.

**Now** a successful `dz build` or `dz run` records each module in
`build/obj/dz/modules-<key>/`, under the build stamp's key (T-0192). A record holds:
- a hash of the module's manifest and sources, or of an imported header's declarations;
- the files it wrote, with their sizes and times;
- the instance bodies it placed, and which it defines;
- whether it read the type table;
- its ownership warnings.

The next build still runs the front end over the whole program: parsing, macros, names,
types, and placement, whose whole-program passes read every function's placement. Then it
finishes only the **affected** modules. A module is affected when its record is missing or
differs, or one of its files was changed or removed, or the same holds for anything it
depends on. For the rest:
- their files in `build/cpp` are compiled as they are;
- their warnings are printed from their records;
- `std` is always instantiated and emitted, since its files key the shared cache.

**Instance bodies** go to the first module in order that needs them (T-0185). The emitter
is told which bodies the kept modules define and defers to an earlier one. After emitting,
every body's owner is worked out again from all modules' needs. A kept module whose bodies
changed hands is instantiated and emitted too. Needs do not depend on ownership, so one more
round settles it, and anything the rule cannot explain emits the whole program.

**Reflection** numbers types in the order modules first name them, so a program that reads
the type table is emitted whole.

**Switches:**
- `DZ_INCREMENTAL=0` turns this off.
- `DZ_INCREMENTAL=verify` also emits the whole program and fails, naming the files, if
  anything kept or written differs. The edit corpus (T-0193) builds incrementally with it
  and compares with a clean build. `crates/dz/tests/reuse.rs` changes a kept file by hand,
  keeping its size and time, and checks a normal build keeps it and a verified one refuses
  it.

**Measured**, a leaf edit at 100 modules (clang++, release `dz`): the whole build 2.45 s
to 0.88 s. Ownership 0.30 s to 6 ms, instantiation 0.23 s to 5 ms, emitting 0.70 s to
19 ms. Placement (0.29 s) and the front end (0.17 s) remain.

**Left for stage 2b:** an edit to a module others depend on still re-emits every dependent,
even when it changed only a function's body.
