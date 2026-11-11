---
id: T-0195
title: The module being edited compiles from its own header, precompiled
milestone: M15
status: done
kind: feature
area: tooling
where: `compile_and_link` in `crates/dz/src/build.rs` (`set_of`, `editing`, the `before` object reused), `start_of` (now also returns the module header), `Pch::prepare`/`spawn` (the header written only when built); test `a_module_being_edited_compiles_from_its_own_header_precompiled` in `crates/dz/tests/run.rs`
---

**Done** (2026-11-11), stage 3b of the incremental-build plan. **What was wrong:** an edit
to one module compiled its `.cpp` again, and most of that file is the module headers beneath
it. In the leaf of `bench_scale.py`'s 100-module project, `#include "m099.h"` alone is
129 000 lines on top of the precompiled `std.h`, 0.47 s of the file's 0.52 s.

**Now** a module's file can start from the module's own header, precompiled. That header
includes the runtime, `std`, the module's C headers and every module header beneath it, and
a body edit leaves it as it was. It is built on the same terms as T-0181's header sets:

- **Built alongside** the compile, not before it. A file compiled before and compiled again
  now (its object exists and is stale) means the module is being edited, and then its
  header is built.
- **Used from the next build on**, while it is fresh: no input changed, and none is newer
  than it. A changed signature in the module, or in any module beneath it, makes it stale.
  Then the file compiles from `std.h`'s or its set's header, and the module's header is
  built again.
- **Never on a build from clean.** No object exists yet, so no module header is built, and
  no folder is written for one: `Pch::prepare` now only names a header, and `spawn` writes
  it.
- **The previous object is reused** when the file has not changed since that compile, as it
  already was for a set's header.

g++ cannot chain a precompiled header on another, so each module's header compiles `std.h`
again inside it. That is the same as a set's.

**Measured**, a leaf edit (C++ compiler phase, release `dz`), from the second edit on:

| | before | after |
|---|---:|---:|
| 100 modules, clang++ | 0.91 s | 0.24 s |
| 10 modules, clang++ | 0.32 s | 0.20 s |
| 10 modules, g++ | 0.43 s | 0.14 s |

The first edit after a clean build compiles as before, while the header builds alongside.
