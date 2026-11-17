| Program | Compiler | Dazzle | C++ | Dazzle / C++ | Peak memory (Dazzle / C++) | Build from clean: Dazzle / C++ |
|---|---|---:|---:|---:|---:|---:|
| astar | clang++ | 325 ms | 414 ms | 0.78× | 9.5 MiB / 12.7 MiB | 0.55 s / 0.45 s |
| astar | g++ | 351 ms | 396 ms | 0.89× | 9.4 MiB / 12.7 MiB | 0.68 s / 0.39 s |
| ecs | clang++ | 130 ms | 186 ms | 0.70× | 3.7 MiB / 3.8 MiB | 1.11 s / 0.74 s |
| ecs | g++ | 126 ms | 125 ms | 1.01× | 3.7 MiB / 3.8 MiB | 1.30 s / 0.78 s |
| expr | clang++ | 129 ms | 134 ms | 0.97× | 141.0 MiB / 150.6 MiB | 0.52 s / 0.56 s |
| expr | g++ | 127 ms | 146 ms | 0.87× | 140.9 MiB / 150.6 MiB | 0.75 s / 0.50 s |
| json | clang++ | 207 ms | 224 ms | 0.92× | 184.3 MiB / 180.0 MiB | 1.05 s / 0.71 s |
| json | g++ | 211 ms | 209 ms | 1.01× | 184.9 MiB / 179.0 MiB | 1.11 s / 0.70 s |
| raytrace | clang++ | 480 ms | 455 ms | 1.05× | 4.8 MiB / 5.0 MiB | 0.34 s / 0.35 s |
| raytrace | g++ | 595 ms | 625 ms | 0.95× | 4.8 MiB / 5.1 MiB | 0.45 s / 0.30 s |
| words | clang++ | 133 ms | 155 ms | 0.86× | 33.6 MiB / 32.1 MiB | 0.47 s / 0.42 s |
| words | g++ | 144 ms | 166 ms | 0.87× | 33.9 MiB / 32.4 MiB | 0.64 s / 0.42 s |

Build time, Dazzle / C++:

| Program | Compiler | debug, from clean | debug, one-line change | release, from clean | release, one-line change |
|---|---|---:|---:|---:|---:|
| astar | clang++ | 0.28 s / 0.31 s | 0.17 s / 0.31 s | 0.55 s / 0.45 s | 0.38 s / 0.45 s |
| astar | g++ | 0.32 s / 0.32 s | 0.17 s / 0.32 s | 0.68 s / 0.39 s | 0.58 s / 0.39 s |
| ecs | clang++ | 0.60 s / 0.51 s | 0.29 s / 0.51 s | 1.11 s / 0.74 s | 0.72 s / 0.73 s |
| ecs | g++ | 0.72 s / 0.59 s | 0.28 s / 0.59 s | 1.30 s / 0.78 s | 1.04 s / 0.77 s |
| expr | clang++ | 0.22 s / 0.42 s | 0.20 s / 0.42 s | 0.52 s / 0.56 s | 0.50 s / 0.56 s |
| expr | g++ | 0.23 s / 0.47 s | 0.21 s / 0.47 s | 0.75 s / 0.50 s | 0.72 s / 0.50 s |
| json | clang++ | 0.31 s / 0.40 s | 0.29 s / 0.40 s | 1.05 s / 0.71 s | 1.02 s / 0.72 s |
| json | g++ | 0.36 s / 0.47 s | 0.33 s / 0.47 s | 1.11 s / 0.70 s | 1.09 s / 0.70 s |
| raytrace | clang++ | 0.19 s / 0.28 s | 0.17 s / 0.28 s | 0.34 s / 0.35 s | 0.32 s / 0.34 s |
| raytrace | g++ | 0.18 s / 0.25 s | 0.16 s / 0.25 s | 0.45 s / 0.30 s | 0.43 s / 0.30 s |
| words | clang++ | 0.22 s / 0.28 s | 0.17 s / 0.28 s | 0.47 s / 0.42 s | 0.39 s / 0.42 s |
| words | g++ | 0.23 s / 0.28 s | 0.16 s / 0.27 s | 0.64 s / 0.42 s | 0.60 s / 0.42 s |

Where Dazzle's build time goes, from `dz build --timings`:

| Program | Compiler | Build | Dazzle's own phases | C and C++ headers | C++ compiler and link | Other |
|---|---|---|---:|---:|---:|---:|
| astar | clang++ | debug, from clean | 73 ms | 2 ms | 199 ms | 8 ms |
| astar | clang++ | debug, one-line change | 56 ms | 1 ms | 108 ms | 8 ms |
| astar | clang++ | release, from clean | 72 ms | 1 ms | 464 ms | 8 ms |
| astar | clang++ | release, one-line change | 56 ms | 1 ms | 314 ms | 6 ms |
| astar | g++ | debug, from clean | 72 ms | 1 ms | 240 ms | 8 ms |
| astar | g++ | debug, one-line change | 55 ms | 2 ms | 100 ms | 10 ms |
| astar | g++ | release, from clean | 71 ms | 1 ms | 602 ms | 7 ms |
| astar | g++ | release, one-line change | 56 ms | 1 ms | 512 ms | 9 ms |
| ecs | clang++ | debug, from clean | 170 ms | 1 ms | 421 ms | 9 ms |
| ecs | clang++ | debug, one-line change | 167 ms | 1 ms | 110 ms | 11 ms |
| ecs | clang++ | release, from clean | 169 ms | 2 ms | 938 ms | 5 ms |
| ecs | clang++ | release, one-line change | 169 ms | 1 ms | 543 ms | 6 ms |
| ecs | g++ | debug, from clean | 169 ms | 1 ms | 540 ms | 9 ms |
| ecs | g++ | debug, one-line change | 166 ms | 1 ms | 104 ms | 9 ms |
| ecs | g++ | release, from clean | 171 ms | 1 ms | 1116 ms | 10 ms |
| ecs | g++ | release, one-line change | 168 ms | 2 ms | 861 ms | 5 ms |
| expr | clang++ | debug, from clean | 66 ms | 1 ms | 146 ms | 9 ms |
| expr | clang++ | debug, one-line change | 49 ms | 1 ms | 142 ms | 7 ms |
| expr | clang++ | release, from clean | 65 ms | 1 ms | 448 ms | 7 ms |
| expr | clang++ | release, one-line change | 50 ms | 1 ms | 445 ms | 7 ms |
| expr | g++ | debug, from clean | 64 ms | 1 ms | 160 ms | 8 ms |
| expr | g++ | debug, one-line change | 49 ms | 1 ms | 155 ms | 8 ms |
| expr | g++ | release, from clean | 64 ms | 1 ms | 672 ms | 9 ms |
| expr | g++ | release, one-line change | 49 ms | 1 ms | 666 ms | 7 ms |
| json | clang++ | debug, from clean | 75 ms | 1 ms | 225 ms | 8 ms |
| json | clang++ | debug, one-line change | 75 ms | 1 ms | 208 ms | 9 ms |
| json | clang++ | release, from clean | 76 ms | 1 ms | 967 ms | 5 ms |
| json | clang++ | release, one-line change | 75 ms | 1 ms | 940 ms | 7 ms |
| json | g++ | debug, from clean | 75 ms | 1 ms | 280 ms | 8 ms |
| json | g++ | debug, one-line change | 74 ms | 2 ms | 248 ms | 8 ms |
| json | g++ | release, from clean | 75 ms | 1 ms | 1028 ms | 9 ms |
| json | g++ | release, one-line change | 73 ms | 1 ms | 1006 ms | 12 ms |
| raytrace | clang++ | debug, from clean | 61 ms | 1 ms | 118 ms | 7 ms |
| raytrace | clang++ | debug, one-line change | 43 ms | 1 ms | 114 ms | 8 ms |
| raytrace | clang++ | release, from clean | 59 ms | 1 ms | 278 ms | 7 ms |
| raytrace | clang++ | release, one-line change | 44 ms | 1 ms | 271 ms | 7 ms |
| raytrace | g++ | debug, from clean | 59 ms | 1 ms | 108 ms | 6 ms |
| raytrace | g++ | debug, one-line change | 43 ms | 2 ms | 108 ms | 7 ms |
| raytrace | g++ | release, from clean | 59 ms | 1 ms | 386 ms | 6 ms |
| raytrace | g++ | release, one-line change | 43 ms | 1 ms | 379 ms | 9 ms |
| words | clang++ | debug, from clean | 66 ms | 1 ms | 141 ms | 6 ms |
| words | clang++ | debug, one-line change | 50 ms | 1 ms | 106 ms | 8 ms |
| words | clang++ | release, from clean | 65 ms | 1 ms | 396 ms | 7 ms |
| words | clang++ | release, one-line change | 50 ms | 1 ms | 337 ms | 7 ms |
| words | g++ | debug, from clean | 66 ms | 1 ms | 153 ms | 7 ms |
| words | g++ | debug, one-line change | 49 ms | 1 ms | 102 ms | 8 ms |
| words | g++ | release, from clean | 66 ms | 1 ms | 569 ms | 7 ms |
| words | g++ | release, one-line change | 49 ms | 1 ms | 540 ms | 8 ms |

Rust halves, each version of a program a binary of its crate in `rust/`, built with `cargo build --release`:

| Program | Rust version | Run time | Peak memory | Build from clean | Dazzle / Rust, clang++ | Dazzle / Rust, g++ |
|---|---|---:|---:|---:|---:|---:|
| ecs | collect | 241 ms | 2.1 MiB | 1.80 s | 0.54× | 0.53× |
| ecs | disjoint | 113 ms | 2.0 MiB | 1.80 s | 1.15× | 1.12× |
| ecs | cell | 240 ms | 2.0 MiB | 1.80 s | 0.54× | 0.53× |

Run time is the median of 10 runs of each half, alternating. Build time is the median of 3 builds, in debug and in `--release`. From clean, `build/` is deleted first: for Dazzle, `dz build` then compiles the program, and finds the runtime and the standard library already compiled, and `stdlib.h` already imported, in its per-user cache, as a C++ program finds its standard library already built; for C++, one compiler invocation. After a one-line change, a function is appended to the program's source and its number changed before each build, one more build first not counted: Dazzle compiles the program's own generated C++ again and links, and C++ compiles its one file again. Dazzle's own phases are reading the manifests, parsing, macros, resolving names, type checking, ownership, placement, instantiating generics and emitting the C++. Other is what no phase accounts for, mostly starting `dz` and linking the vendored libraries. Peak memory is the largest resident set of any run.

- CPU: AMD Ryzen 7 H 255 w/ Radeon 780M Graphics (16 threads)
- OS: Linux-6.12.117+deb13-amd64-x86_64-with-glibc2.41
- `clang++`: Debian clang version 19.1.7 (3+b1)
- `g++`: g++ (Debian 14.2.0-19) 14.2.0
- `rustc`: rustc 1.99.1 (58cf8f9c3 2026-10-13)
- Mode: `--release`. C++ flags: `-std=c++17 -fno-exceptions -fno-rtti -Wall -Wextra -O2 -flto=auto`; Dazzle's generated code gets the same and `-fwrapv -fno-finite-loops -fno-strict-aliasing`, and with g++ in an optimised build `--param max-inline-insns-auto=70` (T-0190) and `--param inline-unit-growth=100` (T-0234). A program's `cxx_flags.txt` adds flags to both of its halves: `raytrace` and `ecs` have `-ffp-contract=off` (T-0174).
- Dazzle: commit 1b4eee3
- Load average before / after: 1.12 / 1.2
