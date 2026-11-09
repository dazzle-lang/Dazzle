---
id: T-0101
title: Every build imports stdlib.h with libclang again: about 380 ms of each build
milestone: M15
status: done
kind: feature
area: tooling
where: `crates/dz_cppimport/src/disk.rs`; `import` and `remember` in `crates/dz_cppimport/src/clang.rs`; `import_headers` in `crates/dz_project`
---

**Done** (2026-11-09). What an import produces is kept on disk, in `headers/` in the
per-user cache (`dz_project::user_cache_dir`), one JSON file per import. An entry holds the
`Header`, every file the parse read with its size, time and content hash, and its full key:
the in-process cache's (headers, directory, every flag, instantiations), the `dz`
executable's path, size and time, and `LIBCLANG_PATH`. It is used while every file holds
the same contents, by size and time first and by hash where the time changed. On a hit
libclang is not loaded at all. The key does not name the libclang the search would find,
since finding it means loading it, which is the cost this avoids; a new libclang's own
headers are among the files read. `Request::cache` says where, `None` in the importer's
tests. The 64 most recently written entries are kept, and any written within the hour.

On Linux (release `dz`), the "C and C++ headers" phase of `words` went from 115 ms to
2 ms, and of `examples/sdl3` (SDL3, ImGui) from 298 ms to 18 ms. A `dz build` of
`words` with nothing changed takes 240 ms in all.

| Piece | Where |
|---|---|
| The entry: key, check, store, prune | `crates/dz_cppimport/src/disk.rs` |
| Asked before libclang is loaded | `import` in `crates/dz_cppimport/src/clang.rs`, then `remember` |
| The cache folder, passed in | `import_headers` in `crates/dz_project/src/lib.rs`, `Request::cache` |
| Tests | `disk::tests` |

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
