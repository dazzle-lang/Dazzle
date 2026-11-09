---
id: T-0182
title: One compiled standard library shared by every project
milestone: M15
status: open
kind: feature
area: tooling
where: `crates/dz/src/build.rs`, `emit_cpp` (the `#line` renames) and `compile_and_link` (the object cache)
---

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
