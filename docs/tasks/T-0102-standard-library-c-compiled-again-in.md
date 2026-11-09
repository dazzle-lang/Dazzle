---
id: T-0102
title: The standard library's C++ is compiled again in every build
milestone: M15
status: open
kind: feature
area: tooling
where: `crates/dz/src/build.rs`, `compile`; `crates/dz_cpp` (which files `std` becomes)
---

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
