| Program | Compiler | Dazzle | C++ | Dazzle / C++ | Peak memory (Dazzle / C++) | Build from clean: Dazzle / C++ |
|---|---|---:|---:|---:|---:|---:|
| expr | clang++ | 140 ms | 134 ms | 1.04× | 140.9 MiB / 150.7 MiB | 0.54 s / 0.57 s |
| expr | g++ | 126 ms | 147 ms | 0.86× | 141.0 MiB / 150.2 MiB | 0.65 s / 0.51 s |
| astar | clang++ | 331 ms | 414 ms | 0.80× | 9.5 MiB / 12.7 MiB | 0.55 s / 0.45 s |
| astar | g++ | 329 ms | 396 ms | 0.83× | 9.5 MiB / 12.8 MiB | 0.68 s / 0.38 s |
| json | clang++ | 242 ms | 224 ms | 1.08× | 196.4 MiB / 180.1 MiB | 0.98 s / 0.72 s |
| json | g++ | 257 ms | 212 ms | 1.21× | 195.9 MiB / 179.9 MiB | 1.17 s / 0.70 s |
| raytrace | clang++ | 486 ms | 454 ms | 1.07× | 4.9 MiB / 5.0 MiB | 0.36 s / 0.34 s |
| raytrace | g++ | 668 ms | 623 ms | 1.07× | 4.9 MiB / 5.0 MiB | 0.47 s / 0.30 s |
| words | clang++ | 138 ms | 155 ms | 0.89× | 33.6 MiB / 32.3 MiB | 0.49 s / 0.42 s |
| words | g++ | 147 ms | 165 ms | 0.89× | 33.8 MiB / 32.5 MiB | 0.76 s / 0.42 s |

Build time, Dazzle / C++:

| Program | Compiler | debug, from clean | debug, one-line change | release, from clean | release, one-line change |
|---|---|---:|---:|---:|---:|
| expr | clang++ | 0.22 s / 0.42 s | 0.20 s / 0.43 s | 0.54 s / 0.57 s | 0.52 s / 0.57 s |
| expr | g++ | 0.23 s / 0.47 s | 0.21 s / 0.47 s | 0.65 s / 0.51 s | 0.63 s / 0.51 s |
| astar | clang++ | 0.28 s / 0.31 s | 0.17 s / 0.31 s | 0.55 s / 0.45 s | 0.39 s / 0.45 s |
| astar | g++ | 0.32 s / 0.32 s | 0.17 s / 0.31 s | 0.68 s / 0.38 s | 0.58 s / 0.38 s |
| json | clang++ | 0.29 s / 0.41 s | 0.26 s / 0.41 s | 0.98 s / 0.72 s | 0.96 s / 0.72 s |
| json | g++ | 0.31 s / 0.47 s | 0.30 s / 0.47 s | 1.17 s / 0.70 s | 1.15 s / 0.70 s |
| raytrace | clang++ | 0.20 s / 0.28 s | 0.18 s / 0.28 s | 0.36 s / 0.34 s | 0.34 s / 0.34 s |
| raytrace | g++ | 0.18 s / 0.25 s | 0.17 s / 0.25 s | 0.47 s / 0.30 s | 0.45 s / 0.30 s |
| words | clang++ | 0.22 s / 0.28 s | 0.17 s / 0.28 s | 0.49 s / 0.42 s | 0.42 s / 0.42 s |
| words | g++ | 0.23 s / 0.28 s | 0.16 s / 0.27 s | 0.76 s / 0.42 s | 0.72 s / 0.42 s |

Where Dazzle's build time goes, from `dz build --timings`:

| Program | Compiler | Build | Dazzle's own phases | C and C++ headers | C++ compiler and link | Other |
|---|---|---|---:|---:|---:|---:|
| expr | clang++ | debug, from clean | 64 ms | 1 ms | 149 ms | 7 ms |
| expr | clang++ | debug, one-line change | 49 ms | 1 ms | 143 ms | 7 ms |
| expr | clang++ | release, from clean | 63 ms | 1 ms | 470 ms | 9 ms |
| expr | clang++ | release, one-line change | 49 ms | 1 ms | 465 ms | 9 ms |
| expr | g++ | debug, from clean | 62 ms | 2 ms | 158 ms | 7 ms |
| expr | g++ | debug, one-line change | 48 ms | 1 ms | 154 ms | 9 ms |
| expr | g++ | release, from clean | 63 ms | 1 ms | 579 ms | 11 ms |
| expr | g++ | release, one-line change | 48 ms | 1 ms | 573 ms | 7 ms |
| astar | clang++ | debug, from clean | 71 ms | 1 ms | 198 ms | 8 ms |
| astar | clang++ | debug, one-line change | 55 ms | 1 ms | 110 ms | 9 ms |
| astar | clang++ | release, from clean | 69 ms | 1 ms | 472 ms | 7 ms |
| astar | clang++ | release, one-line change | 56 ms | 1 ms | 325 ms | 5 ms |
| astar | g++ | debug, from clean | 71 ms | 1 ms | 235 ms | 8 ms |
| astar | g++ | debug, one-line change | 54 ms | 1 ms | 104 ms | 9 ms |
| astar | g++ | release, from clean | 69 ms | 1 ms | 605 ms | 7 ms |
| astar | g++ | release, one-line change | 54 ms | 2 ms | 520 ms | 7 ms |
| json | clang++ | debug, from clean | 66 ms | 1 ms | 211 ms | 8 ms |
| json | clang++ | debug, one-line change | 65 ms | 1 ms | 187 ms | 10 ms |
| json | clang++ | release, from clean | 65 ms | 1 ms | 903 ms | 6 ms |
| json | clang++ | release, one-line change | 65 ms | 1 ms | 889 ms | 9 ms |
| json | g++ | debug, from clean | 67 ms | 1 ms | 236 ms | 10 ms |
| json | g++ | debug, one-line change | 65 ms | 1 ms | 224 ms | 9 ms |
| json | g++ | release, from clean | 65 ms | 2 ms | 1092 ms | 9 ms |
| json | g++ | release, one-line change | 65 ms | 1 ms | 1077 ms | 8 ms |
| raytrace | clang++ | debug, from clean | 57 ms | 1 ms | 128 ms | 9 ms |
| raytrace | clang++ | debug, one-line change | 43 ms | 1 ms | 125 ms | 8 ms |
| raytrace | clang++ | release, from clean | 57 ms | 1 ms | 290 ms | 7 ms |
| raytrace | clang++ | release, one-line change | 43 ms | 1 ms | 287 ms | 7 ms |
| raytrace | g++ | debug, from clean | 56 ms | 1 ms | 118 ms | 9 ms |
| raytrace | g++ | debug, one-line change | 42 ms | 1 ms | 116 ms | 8 ms |
| raytrace | g++ | release, from clean | 57 ms | 1 ms | 403 ms | 8 ms |
| raytrace | g++ | release, one-line change | 43 ms | 1 ms | 400 ms | 10 ms |
| words | clang++ | debug, from clean | 65 ms | 1 ms | 143 ms | 7 ms |
| words | clang++ | debug, one-line change | 49 ms | 1 ms | 108 ms | 8 ms |
| words | clang++ | release, from clean | 65 ms | 1 ms | 417 ms | 7 ms |
| words | clang++ | release, one-line change | 49 ms | 1 ms | 361 ms | 7 ms |
| words | g++ | debug, from clean | 63 ms | 1 ms | 153 ms | 7 ms |
| words | g++ | debug, one-line change | 48 ms | 1 ms | 101 ms | 7 ms |
| words | g++ | release, from clean | 63 ms | 1 ms | 690 ms | 6 ms |
| words | g++ | release, one-line change | 49 ms | 1 ms | 658 ms | 8 ms |

Run time is the median of 10 runs of each half, alternating. Build time is the median of 3 builds, in debug and in `--release`. From clean, `build/` is deleted first: for Dazzle, `dz build` then compiles the program, and finds the runtime and the standard library already compiled, and `stdlib.h` already imported, in its per-user cache, as a C++ program finds its standard library already built; for C++, one compiler invocation. After a one-line change, a function is appended to the program's source and its number changed before each build, one more build first not counted: Dazzle compiles the program's own generated C++ again and links, and C++ compiles its one file again. Dazzle's own phases are reading the manifests, parsing, macros, resolving names, type checking, ownership, placement, instantiating generics and emitting the C++. Other is what no phase accounts for, mostly starting `dz` and linking the vendored libraries. Peak memory is the largest resident set of any run.

- CPU: AMD Ryzen 7 H 255 w/ Radeon 780M Graphics (16 threads)
- OS: Linux-6.12.116+deb13-amd64-x86_64-with-glibc2.41
- `clang++`: Debian clang version 19.1.7 (3+b1)
- `g++`: g++ (Debian 14.2.0-19) 14.2.0
- Mode: `--release`. C++ flags: `-std=c++17 -fno-exceptions -fno-rtti -Wall -Wextra -O2 -flto=auto`; Dazzle's generated code gets the same and `-fwrapv -fno-finite-loops -fno-strict-aliasing`, and with g++ in an optimised build `--param max-inline-insns-auto=70` (T-0190). A program's `cxx_flags.txt` adds flags to both of its halves: `raytrace` has `-ffp-contract=off` (T-0174).
- Dazzle: commit e37c032 plus uncommitted changes to the compiler or the programs
- Load average before / after: 1.38 / 1.07
