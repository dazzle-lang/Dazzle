| Program | Compiler | Dazzle | C++ | Dazzle / C++ | Peak memory (Dazzle / C++) | Build from clean: Dazzle / C++ |
|---|---|---:|---:|---:|---:|---:|
| astar | clang++ | 326 ms | 414 ms | 0.79× | 9.4 MiB / 12.7 MiB | 0.54 s / 0.44 s |
| astar | g++ | 327 ms | 396 ms | 0.83× | 9.4 MiB / 12.7 MiB | 0.67 s / 0.39 s |
| ecs | clang++ | 281 ms | 208 ms | 1.35× | 3.6 MiB / 3.8 MiB | 1.10 s / 0.74 s |
| ecs | g++ | 161 ms | 115 ms | 1.41× | 3.7 MiB / 3.8 MiB | 1.27 s / 0.78 s |
| expr | clang++ | 129 ms | 134 ms | 0.96× | 141.0 MiB / 150.6 MiB | 0.52 s / 0.56 s |
| expr | g++ | 127 ms | 147 ms | 0.87× | 141.0 MiB / 150.6 MiB | 0.76 s / 0.50 s |
| json | clang++ | 204 ms | 221 ms | 0.92× | 187.5 MiB / 180.2 MiB | 1.01 s / 0.71 s |
| json | g++ | 216 ms | 206 ms | 1.05× | 185.6 MiB / 179.7 MiB | 1.20 s / 0.70 s |
| raytrace | clang++ | 481 ms | 455 ms | 1.06× | 4.8 MiB / 5.0 MiB | 0.34 s / 0.33 s |
| raytrace | g++ | 599 ms | 623 ms | 0.96× | 4.8 MiB / 5.0 MiB | 0.45 s / 0.30 s |
| words | clang++ | 134 ms | 155 ms | 0.87× | 33.7 MiB / 32.2 MiB | 0.46 s / 0.42 s |
| words | g++ | 143 ms | 166 ms | 0.86× | 34.0 MiB / 32.1 MiB | 0.73 s / 0.42 s |

Build time, Dazzle / C++:

| Program | Compiler | debug, from clean | debug, one-line change | release, from clean | release, one-line change |
|---|---|---:|---:|---:|---:|
| astar | clang++ | 0.28 s / 0.31 s | 0.17 s / 0.31 s | 0.54 s / 0.44 s | 0.37 s / 0.45 s |
| astar | g++ | 0.32 s / 0.32 s | 0.17 s / 0.32 s | 0.67 s / 0.39 s | 0.57 s / 0.39 s |
| ecs | clang++ | 0.60 s / 0.50 s | 0.29 s / 0.50 s | 1.10 s / 0.74 s | 0.62 s / 0.74 s |
| ecs | g++ | 0.72 s / 0.60 s | 0.28 s / 0.60 s | 1.27 s / 0.78 s | 0.96 s / 0.78 s |
| expr | clang++ | 0.22 s / 0.42 s | 0.20 s / 0.42 s | 0.52 s / 0.56 s | 0.50 s / 0.56 s |
| expr | g++ | 0.23 s / 0.47 s | 0.21 s / 0.47 s | 0.76 s / 0.50 s | 0.74 s / 0.50 s |
| json | clang++ | 0.31 s / 0.40 s | 0.29 s / 0.40 s | 1.01 s / 0.71 s | 0.99 s / 0.71 s |
| json | g++ | 0.36 s / 0.47 s | 0.33 s / 0.47 s | 1.20 s / 0.70 s | 1.18 s / 0.70 s |
| raytrace | clang++ | 0.19 s / 0.27 s | 0.16 s / 0.27 s | 0.34 s / 0.33 s | 0.32 s / 0.33 s |
| raytrace | g++ | 0.18 s / 0.25 s | 0.16 s / 0.25 s | 0.45 s / 0.30 s | 0.43 s / 0.30 s |
| words | clang++ | 0.21 s / 0.28 s | 0.16 s / 0.28 s | 0.46 s / 0.42 s | 0.39 s / 0.42 s |
| words | g++ | 0.23 s / 0.27 s | 0.16 s / 0.27 s | 0.73 s / 0.42 s | 0.68 s / 0.42 s |

Where Dazzle's build time goes, from `dz build --timings`:

| Program | Compiler | Build | Dazzle's own phases | C and C++ headers | C++ compiler and link | Other |
|---|---|---|---:|---:|---:|---:|
| astar | clang++ | debug, from clean | 72 ms | 1 ms | 197 ms | 8 ms |
| astar | clang++ | debug, one-line change | 55 ms | 1 ms | 105 ms | 9 ms |
| astar | clang++ | release, from clean | 73 ms | 1 ms | 457 ms | 7 ms |
| astar | clang++ | release, one-line change | 56 ms | 1 ms | 306 ms | 8 ms |
| astar | g++ | debug, from clean | 71 ms | 2 ms | 238 ms | 9 ms |
| astar | g++ | debug, one-line change | 56 ms | 1 ms | 101 ms | 8 ms |
| astar | g++ | release, from clean | 70 ms | 1 ms | 590 ms | 9 ms |
| astar | g++ | release, one-line change | 55 ms | 2 ms | 504 ms | 6 ms |
| ecs | clang++ | debug, from clean | 169 ms | 1 ms | 419 ms | 10 ms |
| ecs | clang++ | debug, one-line change | 169 ms | 1 ms | 108 ms | 9 ms |
| ecs | clang++ | release, from clean | 171 ms | 1 ms | 923 ms | 9 ms |
| ecs | clang++ | release, one-line change | 169 ms | 1 ms | 444 ms | 9 ms |
| ecs | g++ | debug, from clean | 170 ms | 2 ms | 542 ms | 8 ms |
| ecs | g++ | debug, one-line change | 169 ms | 1 ms | 102 ms | 12 ms |
| ecs | g++ | release, from clean | 169 ms | 1 ms | 1089 ms | 8 ms |
| ecs | g++ | release, one-line change | 167 ms | 1 ms | 779 ms | 11 ms |
| expr | clang++ | debug, from clean | 65 ms | 2 ms | 146 ms | 8 ms |
| expr | clang++ | debug, one-line change | 50 ms | 1 ms | 139 ms | 8 ms |
| expr | clang++ | release, from clean | 64 ms | 1 ms | 446 ms | 7 ms |
| expr | clang++ | release, one-line change | 49 ms | 1 ms | 440 ms | 7 ms |
| expr | g++ | debug, from clean | 64 ms | 1 ms | 160 ms | 8 ms |
| expr | g++ | debug, one-line change | 48 ms | 1 ms | 152 ms | 8 ms |
| expr | g++ | release, from clean | 64 ms | 2 ms | 686 ms | 8 ms |
| expr | g++ | release, one-line change | 48 ms | 1 ms | 682 ms | 9 ms |
| json | clang++ | debug, from clean | 75 ms | 2 ms | 223 ms | 7 ms |
| json | clang++ | debug, one-line change | 74 ms | 1 ms | 203 ms | 11 ms |
| json | clang++ | release, from clean | 75 ms | 1 ms | 928 ms | 8 ms |
| json | clang++ | release, one-line change | 74 ms | 1 ms | 909 ms | 8 ms |
| json | g++ | debug, from clean | 75 ms | 2 ms | 280 ms | 8 ms |
| json | g++ | debug, one-line change | 73 ms | 1 ms | 248 ms | 10 ms |
| json | g++ | release, from clean | 75 ms | 1 ms | 1115 ms | 8 ms |
| json | g++ | release, one-line change | 72 ms | 1 ms | 1096 ms | 11 ms |
| raytrace | clang++ | debug, from clean | 59 ms | 1 ms | 116 ms | 9 ms |
| raytrace | clang++ | debug, one-line change | 44 ms | 2 ms | 113 ms | 7 ms |
| raytrace | clang++ | release, from clean | 59 ms | 1 ms | 273 ms | 7 ms |
| raytrace | clang++ | release, one-line change | 44 ms | 1 ms | 270 ms | 7 ms |
| raytrace | g++ | debug, from clean | 58 ms | 1 ms | 111 ms | 8 ms |
| raytrace | g++ | debug, one-line change | 44 ms | 1 ms | 107 ms | 9 ms |
| raytrace | g++ | release, from clean | 60 ms | 1 ms | 384 ms | 7 ms |
| raytrace | g++ | release, one-line change | 44 ms | 1 ms | 376 ms | 8 ms |
| words | clang++ | debug, from clean | 65 ms | 1 ms | 139 ms | 7 ms |
| words | clang++ | debug, one-line change | 49 ms | 1 ms | 104 ms | 8 ms |
| words | clang++ | release, from clean | 65 ms | 1 ms | 392 ms | 6 ms |
| words | clang++ | release, one-line change | 50 ms | 1 ms | 334 ms | 7 ms |
| words | g++ | debug, from clean | 66 ms | 2 ms | 153 ms | 9 ms |
| words | g++ | debug, one-line change | 50 ms | 2 ms | 100 ms | 8 ms |
| words | g++ | release, from clean | 65 ms | 1 ms | 652 ms | 8 ms |
| words | g++ | release, one-line change | 49 ms | 1 ms | 620 ms | 7 ms |

Rust halves, each version of a program a binary of its crate in `rust/`, built with `cargo build --release`:

| Program | Rust version | Run time | Peak memory | Build from clean | Dazzle / Rust, clang++ | Dazzle / Rust, g++ |
|---|---|---:|---:|---:|---:|---:|
| ecs | collect | 243 ms | 2.1 MiB | 1.80 s | 1.16× | 0.67× |
| ecs | disjoint | 113 ms | 2.0 MiB | 1.80 s | 2.49× | 1.43× |
| ecs | cell | 243 ms | 2.0 MiB | 1.80 s | 1.16× | 0.67× |

Run time is the median of 10 runs of each half, alternating. Build time is the median of 3 builds, in debug and in `--release`. From clean, `build/` is deleted first: for Dazzle, `dz build` then compiles the program, and finds the runtime and the standard library already compiled, and `stdlib.h` already imported, in its per-user cache, as a C++ program finds its standard library already built; for C++, one compiler invocation. After a one-line change, a function is appended to the program's source and its number changed before each build, one more build first not counted: Dazzle compiles the program's own generated C++ again and links, and C++ compiles its one file again. Dazzle's own phases are reading the manifests, parsing, macros, resolving names, type checking, ownership, placement, instantiating generics and emitting the C++. Other is what no phase accounts for, mostly starting `dz` and linking the vendored libraries. Peak memory is the largest resident set of any run.

- CPU: AMD Ryzen 7 H 255 w/ Radeon 780M Graphics (16 threads)
- OS: Linux-6.12.117+deb13-amd64-x86_64-with-glibc2.41
- `clang++`: Debian clang version 19.1.7 (3+b1)
- `g++`: g++ (Debian 14.2.0-19) 14.2.0
- `rustc`: rustc 1.99.1 (58cf8f9c3 2026-10-13)
- Mode: `--release`. C++ flags: `-std=c++17 -fno-exceptions -fno-rtti -Wall -Wextra -O2 -flto=auto`; Dazzle's generated code gets the same and `-fwrapv -fno-finite-loops -fno-strict-aliasing`, and with g++ in an optimised build `--param max-inline-insns-auto=70` (T-0190) and `--param inline-unit-growth=100` (T-0234). A program's `cxx_flags.txt` adds flags to both of its halves: `raytrace` and `ecs` have `-ffp-contract=off` (T-0174).
- Dazzle: commit d238cc7 plus uncommitted changes to the compiler or the programs
- Load average before / after: 1.16 / 1.11
