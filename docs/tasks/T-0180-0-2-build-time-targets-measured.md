---
id: T-0180
title: The 0.2 build-time targets, measured and met
milestone: M15
status: open
kind: feature
area: tooling
where: `tests/bench_programs/RESULTS.md`, `tools/bench_programs.py`; the work is in the tasks below
---

**What it is.** The targets for 0.2, planned with the user on 2026-11-09 and first written
in [T-0102](T-0102-standard-library-c-compiled-again-in.md). Measure the debug build too,
since `dz run` without `--release` is what someone waits on all day. Then aim for:

- a rebuild after a one-line change to a small program (`tests/bench_programs/words`) in
  under 0.5 s in a debug build;
- a build from clean within twice the time of the same program in C++, in release.

Record both, before and after, in `tests/bench_programs/RESULTS.md`, so
[T-0075](T-0075-generic-instantiation-benchmark-pair-tracking-numbers.md)'s tracking starts
from them. The "before" is the `RESULTS.md` at commit f6b62d5 (Linux, `--release`, about
1.4–1.8 s of C++ compiler per program against 0.4–0.6 s for C++).

**Where it stands** (2026-11-09, Windows, clang++ 21, `words`, `dz` itself a debug build):

| | before T-0102 | after T-0102 |
|---|---:|---:|
| debug, one-line change: C++ compile + link | 1.9 s | 0.26 s + 0.08–0.11 s |
| debug, nothing changed: C++ compile + link | 1.9 s | 0.06 s + 0.04 s |
| debug, from clean: C++ compile + link | 1.9–3.8 s | 0.54 s + 0.12 s |

So the C++ side of the first target is met. The whole `dz build` is still about 1.2 s
there, because `dz` was a debug build and its own phases took about 0.85 s; measure with a
release `dz` before calling it met. The clean release target is not met: a clean build still
compiles all of `std`.

**After T-0181** (2026-11-09, the same machine, `dz` a **release** build, three runs each):

| `words`, debug, one-line change | before T-0181 | after T-0181 |
|---|---:|---:|
| clang++ 21: C++ compiler / whole `dz build` | 225 ms / 460 ms | 157 ms / 390 ms |
| g++ 14: C++ compiler / whole `dz build` | 530 ms / 835 ms | 400 ms / 705 ms |

So with a release `dz` and clang++, the first target is met: 0.39 s for the whole rebuild.
With g++ it is not. About 60 ms of the 157 ms is not compiling but checking that objects are
up to date, which hashes every header each object's depfile names: hundreds of them for
`dz_native.cpp`, which includes `windows.h`. The pieces left, each its own task:

1. [T-0181](T-0181-precompiled-header-for-runtime-standard-library.md): a precompiled header.
   **Done** (2026-11-09), the table above. It also precompiles each module's C and C++
   headers, so a one-line change to `examples/sdl3` compiles in 265 ms, from 500 ms. That
   build is still about 1.2 s, mostly importing headers through libclang
   ([T-0101](T-0101-every-build-imports-stdlib-h-with.md)), checking SDL3's CMake build and
   linking.
2. [T-0182](T-0182-one-compiled-standard-library-shared-by.md): one compiled `std` for every
   project, which is what a clean build needs.
3. [T-0183](T-0183-release-build-compiles-everything-twice-where.md): where link-time
   optimisation cannot link, a release build compiles everything twice.
4. [T-0184](T-0184-emit-only-standard-library-declarations-program.md): emit less. The
   largest, and likely unnecessary once 1 and 2 are done.

Run `python tools/bench_programs.py` on the machine that wrote `RESULTS.md` after each.
