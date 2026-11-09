---
id: T-0181
title: A precompiled header for the runtime and the standard library
milestone: M15
status: open
kind: feature
area: tooling
where: `crates/dz/src/build.rs`, `compile_and_link`; `crates/dz_cpp` (what each `.h` includes)
---

**What it is.** Since T-0102 each generated `.cpp` is compiled on its own and the object is
cached, so a one-line change recompiles only the program's own files. But each of those
still parses `dz_runtime.h` and `std.h` (about 0.5 MB for `words`) and then the program's
header (another 0.27 MB). Most of the 0.26 s a one-line rebuild of `words` spends in the C++
compiler is that parse.

**What it would take.** Compile `std.h` (which includes the runtime) once into a
precompiled header in `build/obj/dz/`, keyed and stamped like the objects, and have every
file that includes it use it. clang++ and g++ do this differently: clang takes
`-include-pch std.h.pch`, while g++ finds `std.h.gch` beside the header by itself and
needs the same flags. A precompiled header that does not match is silently ignored by g++,
and is an error in clang. Fall back to no precompiled header, never to a failed build.
Compiler work only, for `docs/log/`. Measure with `dz build --timings` on
`tests/bench_programs/words` after a one-line change.
