| Program | Compiler | Dazzle | C++ | Dazzle / C++ | Peak memory (Dazzle / C++) | Build from clean: Dazzle / C++ |
|---|---|---:|---:|---:|---:|---:|
| astar | clang++ | 330 ms | 412 ms | 0.80× | 9.5 MiB / 12.7 MiB | 0.56 s / 0.45 s |
| astar | g++ | 331 ms | 395 ms | 0.84× | 9.5 MiB / 12.7 MiB | 0.70 s / 0.38 s |
| expr | clang++ | 137 ms | 134 ms | 1.02× | 140.8 MiB / 150.7 MiB | 0.55 s / 0.57 s |
| expr | g++ | 133 ms | 146 ms | 0.91× | 141.1 MiB / 150.7 MiB | 0.80 s / 0.51 s |
| json | clang++ | 239 ms | 223 ms | 1.07× | 208.3 MiB / 180.4 MiB | 1.00 s / 0.72 s |
| json | g++ | 262 ms | 211 ms | 1.24× | 209.9 MiB / 179.6 MiB | 1.17 s / 0.70 s |
| raytrace | clang++ | 486 ms | 454 ms | 1.07× | 4.9 MiB / 5.0 MiB | 0.36 s / 0.34 s |
| raytrace | g++ | 666 ms | 624 ms | 1.07× | 4.9 MiB / 5.0 MiB | 0.47 s / 0.30 s |
| words | clang++ | 136 ms | 154 ms | 0.88× | 33.5 MiB / 32.2 MiB | 0.50 s / 0.42 s |
| words | g++ | 148 ms | 166 ms | 0.89× | 34.0 MiB / 32.3 MiB | 0.76 s / 0.42 s |

Build time, Dazzle / C++:

| Program | Compiler | debug, from clean | debug, one-line change | release, from clean | release, one-line change |
|---|---|---:|---:|---:|---:|
| astar | clang++ | 0.28 s / 0.31 s | 0.17 s / 0.31 s | 0.56 s / 0.45 s | 0.39 s / 0.45 s |
| astar | g++ | 0.32 s / 0.31 s | 0.17 s / 0.31 s | 0.70 s / 0.38 s | 0.60 s / 0.39 s |
| expr | clang++ | 0.22 s / 0.42 s | 0.20 s / 0.42 s | 0.55 s / 0.57 s | 0.53 s / 0.57 s |
| expr | g++ | 0.23 s / 0.47 s | 0.21 s / 0.47 s | 0.80 s / 0.51 s | 0.78 s / 0.51 s |
| json | clang++ | 0.29 s / 0.40 s | 0.26 s / 0.41 s | 1.00 s / 0.72 s | 0.99 s / 0.72 s |
| json | g++ | 0.31 s / 0.47 s | 0.30 s / 0.47 s | 1.17 s / 0.70 s | 1.15 s / 0.70 s |
| raytrace | clang++ | 0.19 s / 0.28 s | 0.18 s / 0.28 s | 0.36 s / 0.34 s | 0.35 s / 0.34 s |
| raytrace | g++ | 0.18 s / 0.25 s | 0.17 s / 0.25 s | 0.47 s / 0.30 s | 0.45 s / 0.30 s |
| words | clang++ | 0.21 s / 0.29 s | 0.16 s / 0.29 s | 0.50 s / 0.42 s | 0.43 s / 0.42 s |
| words | g++ | 0.23 s / 0.28 s | 0.16 s / 0.28 s | 0.76 s / 0.42 s | 0.71 s / 0.42 s |

Where Dazzle's build time goes, from `dz build --timings`:

| Program | Compiler | Build | Dazzle's own phases | C and C++ headers | C++ compiler and link | Other |
|---|---|---|---:|---:|---:|---:|
| astar | clang++ | debug, from clean | 70 ms | 1 ms | 199 ms | 7 ms |
| astar | clang++ | debug, one-line change | 54 ms | 1 ms | 110 ms | 8 ms |
| astar | clang++ | release, from clean | 70 ms | 1 ms | 476 ms | 8 ms |
| astar | clang++ | release, one-line change | 54 ms | 1 ms | 330 ms | 8 ms |
| astar | g++ | debug, from clean | 68 ms | 1 ms | 236 ms | 10 ms |
| astar | g++ | debug, one-line change | 54 ms | 2 ms | 102 ms | 8 ms |
| astar | g++ | release, from clean | 69 ms | 1 ms | 618 ms | 7 ms |
| astar | g++ | release, one-line change | 55 ms | 2 ms | 531 ms | 7 ms |
| expr | clang++ | debug, from clean | 62 ms | 1 ms | 148 ms | 7 ms |
| expr | clang++ | debug, one-line change | 49 ms | 1 ms | 143 ms | 9 ms |
| expr | clang++ | release, from clean | 65 ms | 1 ms | 478 ms | 7 ms |
| expr | clang++ | release, one-line change | 48 ms | 1 ms | 475 ms | 7 ms |
| expr | g++ | debug, from clean | 62 ms | 1 ms | 159 ms | 8 ms |
| expr | g++ | debug, one-line change | 47 ms | 1 ms | 154 ms | 8 ms |
| expr | g++ | release, from clean | 64 ms | 1 ms | 726 ms | 8 ms |
| expr | g++ | release, one-line change | 48 ms | 1 ms | 727 ms | 7 ms |
| json | clang++ | debug, from clean | 66 ms | 1 ms | 209 ms | 8 ms |
| json | clang++ | debug, one-line change | 67 ms | 1 ms | 188 ms | 9 ms |
| json | clang++ | release, from clean | 65 ms | 1 ms | 931 ms | 7 ms |
| json | clang++ | release, one-line change | 66 ms | 1 ms | 916 ms | 9 ms |
| json | g++ | debug, from clean | 65 ms | 1 ms | 237 ms | 9 ms |
| json | g++ | debug, one-line change | 65 ms | 1 ms | 225 ms | 10 ms |
| json | g++ | release, from clean | 65 ms | 1 ms | 1095 ms | 8 ms |
| json | g++ | release, one-line change | 64 ms | 2 ms | 1078 ms | 7 ms |
| raytrace | clang++ | debug, from clean | 58 ms | 1 ms | 126 ms | 7 ms |
| raytrace | clang++ | debug, one-line change | 43 ms | 1 ms | 124 ms | 7 ms |
| raytrace | clang++ | release, from clean | 58 ms | 1 ms | 296 ms | 8 ms |
| raytrace | clang++ | release, one-line change | 42 ms | 1 ms | 294 ms | 9 ms |
| raytrace | g++ | debug, from clean | 57 ms | 1 ms | 118 ms | 8 ms |
| raytrace | g++ | debug, one-line change | 42 ms | 1 ms | 116 ms | 10 ms |
| raytrace | g++ | release, from clean | 56 ms | 1 ms | 406 ms | 7 ms |
| raytrace | g++ | release, one-line change | 44 ms | 1 ms | 403 ms | 7 ms |
| words | clang++ | debug, from clean | 62 ms | 1 ms | 143 ms | 8 ms |
| words | clang++ | debug, one-line change | 48 ms | 2 ms | 108 ms | 7 ms |
| words | clang++ | release, from clean | 64 ms | 1 ms | 428 ms | 6 ms |
| words | clang++ | release, one-line change | 49 ms | 1 ms | 370 ms | 8 ms |
| words | g++ | debug, from clean | 62 ms | 2 ms | 153 ms | 8 ms |
| words | g++ | debug, one-line change | 48 ms | 1 ms | 104 ms | 8 ms |
| words | g++ | release, from clean | 64 ms | 1 ms | 690 ms | 3 ms |
| words | g++ | release, one-line change | 50 ms | 1 ms | 656 ms | 8 ms |

Run time is the median of 10 runs of each half, alternating. Build time is the median of 3 builds, in debug and in `--release`. From clean, `build/` is deleted first: for Dazzle, `dz build` then compiles the program, and finds the runtime and the standard library already compiled, and `stdlib.h` already imported, in its per-user cache, as a C++ program finds its standard library already built; for C++, one compiler invocation. After a one-line change, a function is appended to the program's source and its number changed before each build, one more build first not counted: Dazzle compiles the program's own generated C++ again and links, and C++ compiles its one file again. Dazzle's own phases are reading the manifests, parsing, macros, resolving names, type checking, ownership, placement, instantiating generics and emitting the C++. Other is what no phase accounts for, mostly starting `dz` and linking the vendored libraries. Peak memory is the largest resident set of any run.

- CPU: AMD Ryzen 7 H 255 w/ Radeon 780M Graphics (16 threads)
- OS: Linux-6.12.116+deb13-amd64-x86_64-with-glibc2.41
- `clang++`: Debian clang version 19.1.7 (3+b1)
- `g++`: g++ (Debian 14.2.0-19) 14.2.0
- Mode: `--release`. C++ flags: `-std=c++17 -fno-exceptions -fno-rtti -Wall -Wextra -O2 -flto=auto`; Dazzle's generated code gets the same and `-fwrapv -fno-finite-loops -fno-strict-aliasing`, and with g++ in an optimised build `--param max-inline-insns-auto=70` (T-0190). A program's `cxx_flags.txt` adds flags to both of its halves: `raytrace` has `-ffp-contract=off` (T-0174).
- Dazzle: commit 99dc920 plus uncommitted changes to the compiler or the programs
- Load average before / after: 1.03 / 1.0
