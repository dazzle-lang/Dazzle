| Program | Compiler | Dazzle | C++ | Dazzle / C++ | Peak memory (Dazzle / C++) | Build from clean: Dazzle / C++ |
|---|---|---:|---:|---:|---:|---:|
| astar | clang++ | 327 ms | 414 ms | 0.79× | 9.4 MiB / 12.7 MiB | 0.54 s / 0.45 s |
| astar | g++ | 328 ms | 396 ms | 0.83× | 9.4 MiB / 12.7 MiB | 0.67 s / 0.39 s |
| expr | clang++ | 128 ms | 134 ms | 0.96× | 141.0 MiB / 150.6 MiB | 0.52 s / 0.56 s |
| expr | g++ | 127 ms | 146 ms | 0.87× | 140.7 MiB / 150.1 MiB | 0.77 s / 0.51 s |
| json | clang++ | 213 ms | 223 ms | 0.96× | 187.7 MiB / 180.4 MiB | 0.95 s / 0.72 s |
| json | g++ | 220 ms | 208 ms | 1.06× | 185.8 MiB / 179.8 MiB | 1.08 s / 0.70 s |
| raytrace | clang++ | 480 ms | 454 ms | 1.06× | 4.9 MiB / 5.0 MiB | 0.34 s / 0.34 s |
| raytrace | g++ | 600 ms | 623 ms | 0.96× | 4.8 MiB / 5.0 MiB | 0.45 s / 0.30 s |
| words | clang++ | 134 ms | 155 ms | 0.86× | 33.6 MiB / 32.2 MiB | 0.47 s / 0.42 s |
| words | g++ | 143 ms | 165 ms | 0.86× | 33.8 MiB / 32.2 MiB | 0.73 s / 0.42 s |

Build time, Dazzle / C++:

| Program | Compiler | debug, from clean | debug, one-line change | release, from clean | release, one-line change |
|---|---|---:|---:|---:|---:|
| astar | clang++ | 0.28 s / 0.31 s | 0.18 s / 0.31 s | 0.54 s / 0.45 s | 0.37 s / 0.45 s |
| astar | g++ | 0.32 s / 0.32 s | 0.17 s / 0.31 s | 0.67 s / 0.39 s | 0.57 s / 0.39 s |
| expr | clang++ | 0.22 s / 0.42 s | 0.20 s / 0.42 s | 0.52 s / 0.56 s | 0.50 s / 0.56 s |
| expr | g++ | 0.23 s / 0.47 s | 0.21 s / 0.47 s | 0.77 s / 0.51 s | 0.75 s / 0.51 s |
| json | clang++ | 0.29 s / 0.40 s | 0.26 s / 0.40 s | 0.95 s / 0.72 s | 0.94 s / 0.72 s |
| json | g++ | 0.31 s / 0.47 s | 0.29 s / 0.47 s | 1.08 s / 0.70 s | 1.07 s / 0.70 s |
| raytrace | clang++ | 0.19 s / 0.28 s | 0.16 s / 0.28 s | 0.34 s / 0.34 s | 0.32 s / 0.34 s |
| raytrace | g++ | 0.18 s / 0.25 s | 0.16 s / 0.25 s | 0.45 s / 0.30 s | 0.43 s / 0.30 s |
| words | clang++ | 0.22 s / 0.28 s | 0.17 s / 0.28 s | 0.47 s / 0.42 s | 0.40 s / 0.42 s |
| words | g++ | 0.23 s / 0.28 s | 0.16 s / 0.28 s | 0.73 s / 0.42 s | 0.68 s / 0.42 s |

Where Dazzle's build time goes, from `dz build --timings`:

| Program | Compiler | Build | Dazzle's own phases | C and C++ headers | C++ compiler and link | Other |
|---|---|---|---:|---:|---:|---:|
| astar | clang++ | debug, from clean | 71 ms | 1 ms | 198 ms | 7 ms |
| astar | clang++ | debug, one-line change | 57 ms | 1 ms | 108 ms | 10 ms |
| astar | clang++ | release, from clean | 70 ms | 1 ms | 462 ms | 7 ms |
| astar | clang++ | release, one-line change | 55 ms | 1 ms | 308 ms | 8 ms |
| astar | g++ | debug, from clean | 70 ms | 1 ms | 239 ms | 9 ms |
| astar | g++ | debug, one-line change | 56 ms | 1 ms | 100 ms | 8 ms |
| astar | g++ | release, from clean | 71 ms | 2 ms | 592 ms | 9 ms |
| astar | g++ | release, one-line change | 56 ms | 2 ms | 504 ms | 7 ms |
| expr | clang++ | debug, from clean | 64 ms | 1 ms | 145 ms | 8 ms |
| expr | clang++ | debug, one-line change | 49 ms | 1 ms | 140 ms | 8 ms |
| expr | clang++ | release, from clean | 64 ms | 1 ms | 448 ms | 5 ms |
| expr | clang++ | release, one-line change | 49 ms | 1 ms | 444 ms | 7 ms |
| expr | g++ | debug, from clean | 64 ms | 1 ms | 156 ms | 7 ms |
| expr | g++ | debug, one-line change | 48 ms | 1 ms | 149 ms | 8 ms |
| expr | g++ | release, from clean | 64 ms | 1 ms | 693 ms | 8 ms |
| expr | g++ | release, one-line change | 48 ms | 1 ms | 690 ms | 9 ms |
| json | clang++ | debug, from clean | 67 ms | 1 ms | 211 ms | 8 ms |
| json | clang++ | debug, one-line change | 67 ms | 1 ms | 184 ms | 9 ms |
| json | clang++ | release, from clean | 68 ms | 1 ms | 872 ms | 8 ms |
| json | clang++ | release, one-line change | 67 ms | 1 ms | 860 ms | 7 ms |
| json | g++ | debug, from clean | 67 ms | 1 ms | 231 ms | 8 ms |
| json | g++ | debug, one-line change | 66 ms | 1 ms | 220 ms | 8 ms |
| json | g++ | release, from clean | 67 ms | 1 ms | 1010 ms | 7 ms |
| json | g++ | release, one-line change | 65 ms | 1 ms | 996 ms | 9 ms |
| raytrace | clang++ | debug, from clean | 60 ms | 1 ms | 117 ms | 8 ms |
| raytrace | clang++ | debug, one-line change | 43 ms | 1 ms | 113 ms | 7 ms |
| raytrace | clang++ | release, from clean | 59 ms | 1 ms | 276 ms | 7 ms |
| raytrace | clang++ | release, one-line change | 44 ms | 1 ms | 270 ms | 7 ms |
| raytrace | g++ | debug, from clean | 59 ms | 1 ms | 109 ms | 7 ms |
| raytrace | g++ | debug, one-line change | 42 ms | 1 ms | 108 ms | 7 ms |
| raytrace | g++ | release, from clean | 60 ms | 1 ms | 380 ms | 7 ms |
| raytrace | g++ | release, one-line change | 44 ms | 1 ms | 377 ms | 8 ms |
| words | clang++ | debug, from clean | 66 ms | 1 ms | 141 ms | 7 ms |
| words | clang++ | debug, one-line change | 49 ms | 1 ms | 107 ms | 8 ms |
| words | clang++ | release, from clean | 65 ms | 1 ms | 397 ms | 7 ms |
| words | clang++ | release, one-line change | 50 ms | 1 ms | 338 ms | 7 ms |
| words | g++ | debug, from clean | 65 ms | 1 ms | 155 ms | 7 ms |
| words | g++ | debug, one-line change | 49 ms | 2 ms | 101 ms | 8 ms |
| words | g++ | release, from clean | 65 ms | 1 ms | 655 ms | 9 ms |
| words | g++ | release, one-line change | 50 ms | 1 ms | 623 ms | 7 ms |

Run time is the median of 10 runs of each half, alternating. Build time is the median of 3 builds, in debug and in `--release`. From clean, `build/` is deleted first: for Dazzle, `dz build` then compiles the program, and finds the runtime and the standard library already compiled, and `stdlib.h` already imported, in its per-user cache, as a C++ program finds its standard library already built; for C++, one compiler invocation. After a one-line change, a function is appended to the program's source and its number changed before each build, one more build first not counted: Dazzle compiles the program's own generated C++ again and links, and C++ compiles its one file again. Dazzle's own phases are reading the manifests, parsing, macros, resolving names, type checking, ownership, placement, instantiating generics and emitting the C++. Other is what no phase accounts for, mostly starting `dz` and linking the vendored libraries. Peak memory is the largest resident set of any run.

- CPU: AMD Ryzen 7 H 255 w/ Radeon 780M Graphics (16 threads)
- OS: Linux-6.12.117+deb13-amd64-x86_64-with-glibc2.41
- `clang++`: Debian clang version 19.1.7 (3+b1)
- `g++`: g++ (Debian 14.2.0-19) 14.2.0
- Mode: `--release`. C++ flags: `-std=c++17 -fno-exceptions -fno-rtti -Wall -Wextra -O2 -flto=auto`; Dazzle's generated code gets the same and `-fwrapv -fno-finite-loops -fno-strict-aliasing`, and with g++ in an optimised build `--param max-inline-insns-auto=70` (T-0190) and `--param inline-unit-growth=100` (T-0234). A program's `cxx_flags.txt` adds flags to both of its halves: `raytrace` has `-ffp-contract=off` (T-0174).
- Dazzle: commit ac6e719 plus uncommitted changes to the compiler or the programs
- Load average before / after: 1.05 / 1.1
