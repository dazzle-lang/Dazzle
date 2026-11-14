| Program | Compiler | Dazzle | C++ | Dazzle / C++ | Peak memory (Dazzle / C++) | Build from clean: Dazzle / C++ |
|---|---|---:|---:|---:|---:|---:|
| astar | clang++ | 331 ms | 413 ms | 0.80× | 9.4 MiB / 12.6 MiB | 0.58 s / 0.45 s |
| astar | g++ | 331 ms | 395 ms | 0.84× | 9.4 MiB / 12.7 MiB | 0.70 s / 0.38 s |
| expr | clang++ | 141 ms | 134 ms | 1.05× | 140.5 MiB / 150.1 MiB | 0.58 s / 0.57 s |
| expr | g++ | 135 ms | 146 ms | 0.92× | 140.9 MiB / 150.6 MiB | 0.81 s / 0.51 s |
| words | clang++ | 139 ms | 155 ms | 0.90× | 37.6 MiB / 32.1 MiB | 0.53 s / 0.42 s |
| words | g++ | 142 ms | 166 ms | 0.85× | 37.7 MiB / 32.3 MiB | 0.78 s / 0.42 s |

Build time, Dazzle / C++:

| Program | Compiler | debug, from clean | debug, one-line change | release, from clean | release, one-line change |
|---|---|---:|---:|---:|---:|
| astar | clang++ | 0.31 s / 0.31 s | 0.23 s / 0.31 s | 0.58 s / 0.45 s | 0.44 s / 0.45 s |
| astar | g++ | 0.32 s / 0.32 s | 0.20 s / 0.31 s | 0.70 s / 0.38 s | 0.62 s / 0.39 s |
| expr | clang++ | 0.25 s / 0.42 s | 0.25 s / 0.42 s | 0.58 s / 0.57 s | 0.58 s / 0.56 s |
| expr | g++ | 0.24 s / 0.47 s | 0.23 s / 0.47 s | 0.81 s / 0.51 s | 0.81 s / 0.51 s |
| words | clang++ | 0.24 s / 0.28 s | 0.22 s / 0.28 s | 0.53 s / 0.42 s | 0.47 s / 0.43 s |
| words | g++ | 0.23 s / 0.28 s | 0.19 s / 0.27 s | 0.78 s / 0.42 s | 0.75 s / 0.42 s |

Where Dazzle's build time goes, from `dz build --timings`:

| Program | Compiler | Build | Dazzle's own phases | C and C++ headers | C++ compiler and link | Other |
|---|---|---|---:|---:|---:|---:|
| astar | clang++ | debug, from clean | 71 ms | 2 ms | 209 ms | 24 ms |
| astar | clang++ | debug, one-line change | 71 ms | 1 ms | 132 ms | 24 ms |
| astar | clang++ | release, from clean | 72 ms | 2 ms | 485 ms | 26 ms |
| astar | clang++ | release, one-line change | 72 ms | 1 ms | 338 ms | 24 ms |
| astar | g++ | debug, from clean | 72 ms | 1 ms | 236 ms | 10 ms |
| astar | g++ | debug, one-line change | 71 ms | 2 ms | 115 ms | 11 ms |
| astar | g++ | release, from clean | 72 ms | 2 ms | 618 ms | 10 ms |
| astar | g++ | release, one-line change | 74 ms | 1 ms | 537 ms | 9 ms |
| expr | clang++ | debug, from clean | 66 ms | 2 ms | 161 ms | 24 ms |
| expr | clang++ | debug, one-line change | 66 ms | 1 ms | 160 ms | 25 ms |
| expr | clang++ | release, from clean | 66 ms | 2 ms | 489 ms | 27 ms |
| expr | clang++ | release, one-line change | 66 ms | 1 ms | 484 ms | 24 ms |
| expr | g++ | debug, from clean | 65 ms | 2 ms | 161 ms | 9 ms |
| expr | g++ | debug, one-line change | 66 ms | 1 ms | 157 ms | 10 ms |
| expr | g++ | release, from clean | 64 ms | 1 ms | 739 ms | 10 ms |
| expr | g++ | release, one-line change | 65 ms | 1 ms | 738 ms | 10 ms |
| words | clang++ | debug, from clean | 66 ms | 2 ms | 154 ms | 23 ms |
| words | clang++ | debug, one-line change | 66 ms | 1 ms | 129 ms | 24 ms |
| words | clang++ | release, from clean | 66 ms | 1 ms | 433 ms | 25 ms |
| words | clang++ | release, one-line change | 67 ms | 1 ms | 375 ms | 25 ms |
| words | g++ | debug, from clean | 66 ms | 2 ms | 155 ms | 10 ms |
| words | g++ | debug, one-line change | 67 ms | 1 ms | 109 ms | 10 ms |
| words | g++ | release, from clean | 66 ms | 1 ms | 700 ms | 9 ms |
| words | g++ | release, one-line change | 66 ms | 1 ms | 668 ms | 10 ms |

Run time is the median of 10 runs of each half, alternating. Build time is the median of 3 builds, in debug and in `--release`. From clean, `build/` is deleted first: for Dazzle, `dz build` then compiles the program, and finds the runtime and the standard library already compiled, and `stdlib.h` already imported, in its per-user cache, as a C++ program finds its standard library already built; for C++, one compiler invocation. After a one-line change, a function is appended to the program's source and its number changed before each build, one more build first not counted: Dazzle compiles the program's own generated C++ again and links, and C++ compiles its one file again. Dazzle's own phases are reading the manifests, parsing, macros, resolving names, type checking, ownership, placement, instantiating generics and emitting the C++. Other is what no phase accounts for, mostly starting `dz` and linking the vendored libraries. Peak memory is the largest resident set of any run.

- CPU: AMD Ryzen 7 H 255 w/ Radeon 780M Graphics (16 threads)
- OS: Linux-6.12.116+deb13-amd64-x86_64-with-glibc2.41
- `clang++`: Debian clang version 19.1.7 (3+b1)
- `g++`: g++ (Debian 14.2.0-19) 14.2.0
- Mode: `--release`. C++ flags: `-std=c++17 -fno-exceptions -fno-rtti -Wall -Wextra -O2 -flto=auto`; Dazzle's generated code gets the same and `-fwrapv -fno-finite-loops -fno-strict-aliasing`, and with g++ in an optimised build `--param max-inline-insns-auto=70` (T-0190).
- Dazzle: commit 68ec5ca
- Load average before / after: 1.67 / 1.17
