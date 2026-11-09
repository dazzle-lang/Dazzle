---
id: T-0181
title: A precompiled header for the runtime and the standard library
milestone: M15
status: done
kind: feature
area: tooling
where: `crates/dz/src/build.rs`: `compile` (the retry without it), `compile_and_link`, `Pch`, `Jobs`, `start_of`
---

**Done** (2026-11-09). `std.h` is precompiled into `build/obj/dz/pch-<key>/std.h.gch`,
beside a short `std.h` that includes the generated one. Each module's `.cpp` and
`dz_main.cpp` are compiled with `-include` of that header, which g++ and clang++ both
resolve to the precompiled form. `std.cpp` and the runtime's files do not use it, and on a
build from clean they compile while it is built. A module whose header goes on to include
C or C++ headers (`<SDL3/SDL.h>`, `<imgui.h>`) gets a second one with those too, one for
each distinct set. It is slower to build, so it is built alongside the first compile that
needs it and used from the next build on. A header that cannot be built or used means a
build without it, with a note.

On Windows, debug, a one-line change now spends this long in the C++ compiler:

| | before | `std.h` only | with library headers |
|---|---:|---:|---:|
| `words`, clang++ 21 | 225 ms | 157 ms | 157 ms (no libraries) |
| `words`, g++ 14 | 530 ms | 400 ms | 400 ms (no libraries) |
| `examples/sdl3` (SDL3, ImGui), clang++ 21 | 500 ms | 430 ms | 265 ms |
| `examples/raylib`, clang++ 21 / g++ 14 | | | 95 ms / 155 ms |

A clean build is no slower. `docs/log/tooling.md` has the details.

| Piece | Where |
|---|---|
| Which files use which header | `start_of` and `Start` in `crates/dz/src/build.rs` |
| Building one, and when it is fresh | `Pch::prepare`, `Pch::fresh`, `Pch::spawn` |
| Compiling the files around them | `compile_and_link`, and the compile pool `Jobs` |
| Building again without them | `compile`, after the LTO fallback (`Failed::FromPch`) |

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
