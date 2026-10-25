---
id: T-0101
title: Every build imports stdlib.h with libclang again: about 380 ms of each build
milestone: M15
status: open
kind: feature
area: tooling
where: `crates/dz_project/src/lib.rs`, `load_for` and `import_headers`; `crates/dz_cppimport`
---

**What it is.** The standard library imports `stdlib.h` for `Allocators::Global` (spec
§14.2), so every program imports at least one C header, through libclang. `dz build
--timings` shows what that costs: on the three programs in `tests/bench_programs`, on a
Ryzen 7 H 255, it takes about 380 ms, while Dazzle's own phases together (parsing through
emitting the C++) take about 35 ms. The same import happens in every `dz check`, `dz build`
and `dz run`, because each one is a new process. The language server keeps the import
between analyses, but only in its own memory.

**Why it matters.** Principle 6 is "compile fast". A `dz check` that takes 430 ms where
35 ms is Dazzle's own work feels slow for reasons that have nothing to do with the
language, and a program that imports a large header (SDL, raylib) pays more.

**What it would take.** Compiler work, no language change. Cache an imported header's
declarations on disk, keyed by what decides them: the header's path and content hash (and
every file it includes), the include directories, the defines, the C++ standard and the
libclang version. A later build reads the cache instead of running libclang. The key has
to be complete: a stale cache gives wrong declarations, silently, which is worse than a
slow build. Re-run `python tools/bench_programs.py` before and after, since it reports this
phase.
