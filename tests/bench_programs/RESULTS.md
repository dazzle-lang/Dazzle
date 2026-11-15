| Program | Compiler | Dazzle | C++ | Dazzle / C++ | Peak memory (Dazzle / C++) | Build from clean: Dazzle / C++ |
|---|---|---:|---:|---:|---:|---:|
| astar | clang++ | 333 ms | 413 ms | 0.81× | 9.5 MiB / 12.7 MiB | 0.55 s / 0.45 s |
| astar | g++ | 329 ms | 396 ms | 0.83× | 9.6 MiB / 12.7 MiB | 0.69 s / 0.39 s |
| expr | clang++ | 139 ms | 134 ms | 1.04× | 141.1 MiB / 150.7 MiB | 0.54 s / 0.57 s |
| expr | g++ | 127 ms | 146 ms | 0.87× | 140.8 MiB / 150.3 MiB | 0.66 s / 0.50 s |
| json | clang++ | 223 ms | 225 ms | 0.99× | 184.7 MiB / 179.8 MiB | 0.96 s / 0.72 s |
| json | g++ | 223 ms | 210 ms | 1.07× | 187.1 MiB / 180.1 MiB | 1.09 s / 0.70 s |
| raytrace | clang++ | 479 ms | 454 ms | 1.06× | 4.9 MiB / 5.0 MiB | 0.35 s / 0.34 s |
| raytrace | g++ | 602 ms | 623 ms | 0.97× | 4.9 MiB / 5.0 MiB | 0.46 s / 0.30 s |
| words | clang++ | 136 ms | 155 ms | 0.88× | 33.8 MiB / 32.2 MiB | 0.48 s / 0.42 s |
| words | g++ | 146 ms | 166 ms | 0.88× | 34.0 MiB / 32.5 MiB | 0.75 s / 0.42 s |

Build time, Dazzle / C++:

| Program | Compiler | debug, from clean | debug, one-line change | release, from clean | release, one-line change |
|---|---|---:|---:|---:|---:|
| astar | clang++ | 0.28 s / 0.32 s | 0.17 s / 0.31 s | 0.55 s / 0.45 s | 0.39 s / 0.45 s |
| astar | g++ | 0.32 s / 0.32 s | 0.16 s / 0.32 s | 0.69 s / 0.39 s | 0.58 s / 0.38 s |
| expr | clang++ | 0.22 s / 0.42 s | 0.20 s / 0.42 s | 0.54 s / 0.57 s | 0.51 s / 0.57 s |
| expr | g++ | 0.23 s / 0.47 s | 0.21 s / 0.47 s | 0.66 s / 0.50 s | 0.63 s / 0.51 s |
| json | clang++ | 0.29 s / 0.41 s | 0.26 s / 0.41 s | 0.96 s / 0.72 s | 0.95 s / 0.72 s |
| json | g++ | 0.31 s / 0.47 s | 0.30 s / 0.47 s | 1.09 s / 0.70 s | 1.08 s / 0.70 s |
| raytrace | clang++ | 0.19 s / 0.28 s | 0.17 s / 0.28 s | 0.35 s / 0.34 s | 0.33 s / 0.34 s |
| raytrace | g++ | 0.18 s / 0.25 s | 0.16 s / 0.25 s | 0.46 s / 0.30 s | 0.44 s / 0.30 s |
| words | clang++ | 0.22 s / 0.29 s | 0.17 s / 0.29 s | 0.48 s / 0.42 s | 0.41 s / 0.42 s |
| words | g++ | 0.23 s / 0.28 s | 0.16 s / 0.27 s | 0.75 s / 0.42 s | 0.70 s / 0.42 s |

Where Dazzle's build time goes, from `dz build --timings`:

| Program | Compiler | Build | Dazzle's own phases | C and C++ headers | C++ compiler and link | Other |
|---|---|---|---:|---:|---:|---:|
| astar | clang++ | debug, from clean | 71 ms | 1 ms | 200 ms | 8 ms |
| astar | clang++ | debug, one-line change | 56 ms | 1 ms | 109 ms | 7 ms |
| astar | clang++ | release, from clean | 72 ms | 1 ms | 473 ms | 8 ms |
| astar | clang++ | release, one-line change | 56 ms | 1 ms | 320 ms | 9 ms |
| astar | g++ | debug, from clean | 69 ms | 1 ms | 239 ms | 8 ms |
| astar | g++ | debug, one-line change | 55 ms | 2 ms | 100 ms | 7 ms |
| astar | g++ | release, from clean | 71 ms | 1 ms | 607 ms | 8 ms |
| astar | g++ | release, one-line change | 56 ms | 1 ms | 516 ms | 9 ms |
| expr | clang++ | debug, from clean | 64 ms | 1 ms | 146 ms | 6 ms |
| expr | clang++ | debug, one-line change | 49 ms | 1 ms | 142 ms | 7 ms |
| expr | clang++ | release, from clean | 64 ms | 1 ms | 464 ms | 8 ms |
| expr | clang++ | release, one-line change | 50 ms | 1 ms | 457 ms | 6 ms |
| expr | g++ | debug, from clean | 64 ms | 1 ms | 159 ms | 9 ms |
| expr | g++ | debug, one-line change | 48 ms | 1 ms | 154 ms | 7 ms |
| expr | g++ | release, from clean | 63 ms | 1 ms | 588 ms | 7 ms |
| expr | g++ | release, one-line change | 49 ms | 1 ms | 578 ms | 7 ms |
| json | clang++ | debug, from clean | 67 ms | 2 ms | 214 ms | 7 ms |
| json | clang++ | debug, one-line change | 66 ms | 1 ms | 188 ms | 8 ms |
| json | clang++ | release, from clean | 68 ms | 1 ms | 881 ms | 6 ms |
| json | clang++ | release, one-line change | 67 ms | 2 ms | 871 ms | 10 ms |
| json | g++ | debug, from clean | 66 ms | 1 ms | 233 ms | 7 ms |
| json | g++ | debug, one-line change | 66 ms | 1 ms | 224 ms | 8 ms |
| json | g++ | release, from clean | 68 ms | 1 ms | 1019 ms | 6 ms |
| json | g++ | release, one-line change | 66 ms | 1 ms | 1006 ms | 8 ms |
| raytrace | clang++ | debug, from clean | 58 ms | 1 ms | 119 ms | 8 ms |
| raytrace | clang++ | debug, one-line change | 43 ms | 1 ms | 116 ms | 7 ms |
| raytrace | clang++ | release, from clean | 58 ms | 1 ms | 287 ms | 7 ms |
| raytrace | clang++ | release, one-line change | 44 ms | 1 ms | 279 ms | 9 ms |
| raytrace | g++ | debug, from clean | 57 ms | 2 ms | 109 ms | 7 ms |
| raytrace | g++ | debug, one-line change | 43 ms | 1 ms | 107 ms | 8 ms |
| raytrace | g++ | release, from clean | 58 ms | 1 ms | 394 ms | 7 ms |
| raytrace | g++ | release, one-line change | 44 ms | 2 ms | 389 ms | 9 ms |
| words | clang++ | debug, from clean | 66 ms | 1 ms | 144 ms | 7 ms |
| words | clang++ | debug, one-line change | 50 ms | 1 ms | 109 ms | 8 ms |
| words | clang++ | release, from clean | 65 ms | 1 ms | 408 ms | 7 ms |
| words | clang++ | release, one-line change | 51 ms | 1 ms | 352 ms | 7 ms |
| words | g++ | debug, from clean | 64 ms | 1 ms | 155 ms | 7 ms |
| words | g++ | debug, one-line change | 50 ms | 1 ms | 103 ms | 7 ms |
| words | g++ | release, from clean | 65 ms | 1 ms | 671 ms | 9 ms |
| words | g++ | release, one-line change | 51 ms | 2 ms | 642 ms | 7 ms |

Run time is the median of 10 runs of each half, alternating. Build time is the median of 3 builds, in debug and in `--release`. From clean, `build/` is deleted first: for Dazzle, `dz build` then compiles the program, and finds the runtime and the standard library already compiled, and `stdlib.h` already imported, in its per-user cache, as a C++ program finds its standard library already built; for C++, one compiler invocation. After a one-line change, a function is appended to the program's source and its number changed before each build, one more build first not counted: Dazzle compiles the program's own generated C++ again and links, and C++ compiles its one file again. Dazzle's own phases are reading the manifests, parsing, macros, resolving names, type checking, ownership, placement, instantiating generics and emitting the C++. Other is what no phase accounts for, mostly starting `dz` and linking the vendored libraries. Peak memory is the largest resident set of any run.

- CPU: AMD Ryzen 7 H 255 w/ Radeon 780M Graphics (16 threads)
- OS: Linux-6.12.116+deb13-amd64-x86_64-with-glibc2.41
- `clang++`: Debian clang version 19.1.7 (3+b1)
- `g++`: g++ (Debian 14.2.0-19) 14.2.0
- Mode: `--release`. C++ flags: `-std=c++17 -fno-exceptions -fno-rtti -Wall -Wextra -O2 -flto=auto`; Dazzle's generated code gets the same and `-fwrapv -fno-finite-loops -fno-strict-aliasing`, and with g++ in an optimised build `--param max-inline-insns-auto=70` (T-0190) and `--param inline-unit-growth=100` (T-0234). A program's `cxx_flags.txt` adds flags to both of its halves: `raytrace` has `-ffp-contract=off` (T-0174).
- Dazzle: commit e2fb6f3 plus uncommitted changes to the compiler or the programs
- Load average before / after: 1.91 / 1.55
