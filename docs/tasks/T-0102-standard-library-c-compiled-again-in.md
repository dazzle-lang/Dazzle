---
id: T-0102
title: The standard library's C++ is compiled again in every build
milestone: M15
status: done
kind: feature
area: tooling
where: `crates/dz/src/build.rs`, `compile` and `compile_and_link`; `crates/dz/src/native.rs`, `up_to_date` and `write_stamp` (the shared object cache)
---

**Done** (2026-11-09), option 1 below. Each generated `.cpp` becomes its own object in
`build/obj/dz/`, compiled in parallel and reused while its inputs are unchanged. The check
is the vendored libraries' own: the source, every header in the compiler's depfile, and
the command, which is part of the object's name. The objects are then linked by a command
of their own, and `--timings` reports `link` apart from `C++ compiler`. On `words` (debug,
clang++ 21, Windows) a one-line change now spends 0.26 s compiling and 0.1 s linking, where
the single command took 1.9 s. A clean build takes 0.54 s, since the files compile at once.

The targets below moved to [T-0180](T-0180-0-2-build-time-targets-measured.md), with what
is left of them split into
[T-0181](T-0181-precompiled-header-for-runtime-standard-library.md) (a precompiled
header), [T-0182](T-0182-one-compiled-standard-library-shared-by.md) (one `std` for
every project), [T-0183](T-0183-release-build-compiles-everything-twice-where.md) (the LTO
fallback compiling twice) and
[T-0184](T-0184-emit-only-standard-library-declarations-program.md) (option 3).

**What it is.** `dz build` passes every generated `.cpp` file to one C++ compiler command:
the program's, the runtime's, and the standard library's (`std.cpp` with `std.h`; for
`words`, about 1 MB of C++ in all). Only the vendored C and C++ libraries have an object
cache, in `build/obj/`. So the standard library is compiled again in every build, even
when nothing in it changed.

`dz build --timings` on the three programs in `tests/bench_programs` puts the C++
compiler at 1.4–2.4 s per build, at `-O2` on a Ryzen 7 H 255. The same programs written in
C++ compile in 0.3–0.5 s.

**Why it matters.** It is most of the gap between Dazzle's build time and C++'s, and it
does not depend on the program: a ten-line program pays it too. Principle 6 is "compile
fast".

**What it would take.** Compiler work, no language change. Some options, which can be
combined:

1. **Compile each generated file separately** into `build/obj/`, keyed like the vendored
   libraries' objects, so an unchanged standard library is not recompiled. Generic
   instantiations already go to the program's own files; whatever of `std.cpp` still
   depends on the program would have to move there too, or the object changes with every
   program.
2. **A precompiled header** for the runtime and the standard library's declarations.
3. **Emit less**: only the standard library declarations a program reaches.

Measure with `dz build --timings` and `python tools/bench_programs.py`.

**The target for 0.2** (planned with the user on 2026-11-09). Measure the debug build too,
since `dz run` without `--release` is what someone waits on all day, and every figure above
is `-O2` with LTO. Then aim for:

- a rebuild after a one-line change to a small program (`tests/bench_programs/words`) in
  under 0.5 s in a debug build;
- a build from clean within twice the time of the same program in C++, in release.

Record both, before and after, in `tests/bench_programs/RESULTS.md`, so
[T-0075](T-0075-generic-instantiation-benchmark-pair-tracking-numbers.md)'s tracking starts
from them.
