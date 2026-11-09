---
id: T-0184
title: Emit only the standard library declarations a program reaches
milestone: M15
status: open
kind: feature
area: tooling
where: `crates/dz_cpp` (which declarations `std.h` and `std.cpp` hold)
---

**What it is.** Every program gets all of `std` in C++: about 0.9 MB of `std.h` and
`std.cpp`, whatever it uses. Option 3 of T-0102.

**Why it is last.** It cuts against [T-0182](T-0182-one-compiled-standard-library-shared-by.md).
A `std.cpp` trimmed to one program is different for every program, so it cannot be shared.
It is worth doing only if a precompiled header
([T-0181](T-0181-precompiled-header-for-runtime-standard-library.md)) and a shared `std`
together leave the targets of [T-0180](T-0180-0-2-build-time-targets-measured.md) unmet.
Even then, trimming `std.h`, which every file parses, matters more than trimming `std.cpp`,
which is compiled once. Compiler work only, for `docs/log/`.

**2026-11-09:** the targets were met without it (T-0180), so it waits until a larger program
(T-0012) shows `std.h`'s parse is what is left.
