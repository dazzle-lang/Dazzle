---
id: T-0182
title: One compiled standard library shared by every project
milestone: M15
status: done
kind: feature
area: tooling
where: `crates/dz/src/shared.rs`; `crates/dz/src/build.rs`, `emit_cpp` and `compile_and_link`; `GeneratedFile::shared` in `crates/dz_cpp`
---

**Done** (2026-11-09). The runtime's five files and `std`'s `.h` and `.cpp` are marked
`shared` by `dz_cpp`. `emit_cpp` writes them, and the `std` sources their `#line`s name, to
`<cache>/std-<hash>/` in the per-user cache (`~/.cache/dazzle` on Linux, `DZ_CACHE_DIR` to
move it), the hash being of their contents. `compile` compiles them, and precompiles
`std.h`, from there, so every project links the same objects. A setting that changes them
(`--track-memory`, `DZ_REGIONS=0`, `check_trusted`, a harness) gets a folder of its own,
so nothing had to be listed for the key. `build/cpp` still holds a copy of every file.
Since parallel `dz` processes share the folder, every object, depfile, stamp and
precompiled header is written beside its name and renamed onto it. The 8 most recently
used folders are kept, and none used within the hour, since the test suite alone uses
about ten at once.

On Linux (Ryzen 7 H 255, release `dz`, median of three), a build from clean:

| | debug before / after | release before / after | C++ debug / release |
|---|---:|---:|---:|
| `words`, clang++ 19 | 0.76 s / 0.37 s | 1.21 s / 0.61 s | 0.28 s / 0.42 s |
| `words`, g++ 14 | 0.88 s / 0.39 s | 1.11 s / 0.75 s | 0.28 s / 0.42 s |
| `astar`, clang++ 19 | 0.77 s / 0.42 s | 1.17 s / 0.62 s | 0.31 s / 0.45 s |
| `expr`, g++ 14 | 0.88 s / 0.43 s | 1.18 s / 0.82 s | 0.47 s / 0.51 s |

A build from clean now costs what a one-line change does. The first build on a machine, or
the first after `std` changes, still compiles `std` (about 0.75 s in debug).

| Piece | Where |
|---|---|
| Which files are shared | `GeneratedFile::shared`, `crates/dz_cpp/src/lib.rs` |
| The folder: key, layout, pruning, writing | `Shared`, `prune`, `write`, `temp_of` in `crates/dz/src/shared.rs` |
| Where the cache is | `dz_project::user_cache_dir` |
| Writing the files and the `#line` renames | `emit_cpp`, `crates/dz/src/build.rs` |
| Compiling from it | `compile`, `compile_and_link` and `Dirs`; `put_in_place` for the renames |
| Tests | `a_second_project_reuses_the_compiled_standard_library` and `line_directives_name_files_a_debugger_can_open` in `crates/dz/tests/run.rs`; `shared::tests` |

**What it is.** Since T-0102 an unchanged `std` is not compiled again, but only within one
project's `build/`. A clean build, a new project, or `dz test` in a fresh harness compiles
all of `std.cpp` again: about 0.5 s at `-O0` and most of a release build. That is the gap
to the second target of [T-0180](T-0180-0-2-build-time-targets-measured.md).

**Why it is close.** On 2026-11-09, `std.cpp` and `std.h` for `words`, `astar` and `expr`
were byte for byte the same once the `#line` directives were left out. What differs is
the `#line` paths: `emit_cpp` points them at copies of `std`'s sources written into each
project's own `build/cpp/std/`, so a debugger can open them.

**What it would take.** Write those copies, and the compiled objects, to one cache outside
the project, keyed by the `dz` version and the compiler flags (a per-user cache directory,
or one beside `dz` itself), so every project's `std.cpp` names the same paths and has the
same bytes. Check what else can make `std.cpp` differ between programs (`#if` flags,
`--track-memory`, `DZ_REGIONS`); each goes in the key. Compiler work only, for `docs/log/`.
