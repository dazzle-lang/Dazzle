| Program | Compiler | Dazzle | C++ | Dazzle / C++ | Peak memory (Dazzle / C++) | Build from clean: Dazzle / C++ |
|---|---|---:|---:|---:|---:|---:|
| astar | clang++ | 317 ms | 414 ms | 0.77× | 9.4 MiB / 12.6 MiB | 0.70 s / 0.45 s |
| astar | g++ | 330 ms | 396 ms | 0.83× | 9.4 MiB / 12.7 MiB | 0.75 s / 0.39 s |
| expr | clang++ | 157 ms | 134 ms | 1.17× | 141.0 MiB / 150.1 MiB | 0.71 s / 0.57 s |
| expr | g++ | 151 ms | 146 ms | 1.04× | 140.7 MiB / 150.6 MiB | 0.87 s / 0.51 s |
| words | clang++ | 140 ms | 155 ms | 0.90× | 37.6 MiB / 32.0 MiB | 0.60 s / 0.43 s |
| words | g++ | 143 ms | 167 ms | 0.86× | 37.7 MiB / 32.3 MiB | 0.80 s / 0.42 s |

Build time, Dazzle / C++:

| Program | Compiler | debug, from clean | debug, one-line change | release, from clean | release, one-line change |
|---|---|---:|---:|---:|---:|
| astar | clang++ | 0.40 s / 0.32 s | 0.41 s / 0.32 s | 0.70 s / 0.45 s | 0.70 s / 0.45 s |
| astar | g++ | 0.45 s / 0.32 s | 0.45 s / 0.32 s | 0.75 s / 0.39 s | 0.75 s / 0.39 s |
| expr | clang++ | 0.34 s / 0.43 s | 0.35 s / 0.43 s | 0.71 s / 0.57 s | 0.72 s / 0.57 s |
| expr | g++ | 0.38 s / 0.47 s | 0.37 s / 0.47 s | 0.87 s / 0.51 s | 0.88 s / 0.51 s |
| words | clang++ | 0.31 s / 0.29 s | 0.32 s / 0.29 s | 0.60 s / 0.43 s | 0.60 s / 0.43 s |
| words | g++ | 0.33 s / 0.28 s | 0.33 s / 0.27 s | 0.80 s / 0.42 s | 0.80 s / 0.42 s |

Where Dazzle's build time goes, from `dz build --timings`:

| Program | Compiler | Build | Dazzle's own phases | C and C++ headers | C++ compiler and link | Other |
|---|---|---|---:|---:|---:|---:|
| astar | clang++ | debug, from clean | 99 ms | 2 ms | 297 ms | 7 ms |
| astar | clang++ | debug, one-line change | 98 ms | 2 ms | 297 ms | 9 ms |
| astar | clang++ | release, from clean | 98 ms | 1 ms | 572 ms | 25 ms |
| astar | clang++ | release, one-line change | 99 ms | 2 ms | 570 ms | 28 ms |
| astar | g++ | debug, from clean | 100 ms | 2 ms | 343 ms | 9 ms |
| astar | g++ | debug, one-line change | 98 ms | 2 ms | 342 ms | 10 ms |
| astar | g++ | release, from clean | 98 ms | 2 ms | 643 ms | 10 ms |
| astar | g++ | release, one-line change | 98 ms | 2 ms | 642 ms | 11 ms |
| expr | clang++ | debug, from clean | 90 ms | 2 ms | 246 ms | 8 ms |
| expr | clang++ | debug, one-line change | 89 ms | 2 ms | 248 ms | 10 ms |
| expr | clang++ | release, from clean | 90 ms | 2 ms | 597 ms | 25 ms |
| expr | clang++ | release, one-line change | 90 ms | 2 ms | 600 ms | 29 ms |
| expr | g++ | debug, from clean | 90 ms | 2 ms | 277 ms | 8 ms |
| expr | g++ | debug, one-line change | 89 ms | 2 ms | 276 ms | 7 ms |
| expr | g++ | release, from clean | 90 ms | 2 ms | 771 ms | 10 ms |
| expr | g++ | release, one-line change | 89 ms | 2 ms | 774 ms | 11 ms |
| words | clang++ | debug, from clean | 90 ms | 2 ms | 213 ms | 8 ms |
| words | clang++ | debug, one-line change | 90 ms | 2 ms | 216 ms | 8 ms |
| words | clang++ | release, from clean | 89 ms | 2 ms | 484 ms | 26 ms |
| words | clang++ | release, one-line change | 89 ms | 1 ms | 487 ms | 27 ms |
| words | g++ | debug, from clean | 90 ms | 2 ms | 235 ms | 8 ms |
| words | g++ | debug, one-line change | 90 ms | 2 ms | 231 ms | 8 ms |
| words | g++ | release, from clean | 88 ms | 2 ms | 695 ms | 11 ms |
| words | g++ | release, one-line change | 88 ms | 2 ms | 700 ms | 12 ms |

Run time is the median of 10 runs of each half, alternating. Build time is the median of 3 builds, in debug and in `--release`. From clean, `build/` is deleted first: for Dazzle, `dz build` then compiles the program, and finds the runtime and the standard library already compiled, and `stdlib.h` already imported, in its per-user cache, as a C++ program finds its standard library already built; for C++, one compiler invocation. After a one-line change, a function is appended to the program's source and its number changed before each build, one more build first not counted: Dazzle compiles the program's own generated C++ again and links, and C++ compiles its one file again. Dazzle's own phases are reading the manifests, parsing, macros, resolving names, type checking, ownership, placement, instantiating generics and emitting the C++. Other is what no phase accounts for, mostly starting `dz` and linking the vendored libraries. Peak memory is the largest resident set of any run.

- CPU: AMD Ryzen 7 H 255 w/ Radeon 780M Graphics (16 threads)
- OS: Linux-6.12.115+deb13.1-amd64-x86_64-with-glibc2.41
- `clang++`: Debian clang version 19.1.7 (3+b1)
- `g++`: g++ (Debian 14.2.0-19) 14.2.0
- Mode: `--release`. C++ flags: `-std=c++17 -fno-exceptions -fno-rtti -Wall -Wextra -O2 -flto=auto`; Dazzle's generated code gets the same and `-fwrapv -fno-finite-loops -fno-strict-aliasing`, and with g++ in an optimised build `--param max-inline-insns-auto=70` (T-0190).
- Dazzle: commit ecb942b plus uncommitted changes to the compiler or the programs
- Load average before / after: 1.98 / 1.28
