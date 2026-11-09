| Program | Compiler | Dazzle | C++ | Dazzle / C++ | Peak memory (Dazzle / C++) | Build from clean: Dazzle / C++ |
|---|---|---:|---:|---:|---:|---:|
| astar | clang++ | 319 ms | 412 ms | 0.77× | 9.4 MiB / 12.7 MiB | 0.54 s / 0.45 s |
| astar | g++ | 352 ms | 395 ms | 0.89× | 9.5 MiB / 12.6 MiB | 0.61 s / 0.38 s |
| expr | clang++ | 157 ms | 134 ms | 1.17× | 140.9 MiB / 150.1 MiB | 0.65 s / 0.57 s |
| expr | g++ | 157 ms | 146 ms | 1.08× | 140.7 MiB / 150.7 MiB | 0.73 s / 0.51 s |
| words | clang++ | 140 ms | 155 ms | 0.90× | 37.7 MiB / 32.2 MiB | 0.53 s / 0.42 s |
| words | g++ | 168 ms | 166 ms | 1.01× | 37.8 MiB / 32.3 MiB | 0.66 s / 0.42 s |

Build time, Dazzle / C++:

| Program | Compiler | debug, from clean | debug, one-line change | release, from clean | release, one-line change |
|---|---|---:|---:|---:|---:|
| astar | clang++ | 0.34 s / 0.31 s | 0.34 s / 0.31 s | 0.54 s / 0.45 s | 0.54 s / 0.46 s |
| astar | g++ | 0.35 s / 0.32 s | 0.35 s / 0.32 s | 0.61 s / 0.38 s | 0.61 s / 0.38 s |
| expr | clang++ | 0.32 s / 0.43 s | 0.32 s / 0.42 s | 0.65 s / 0.57 s | 0.65 s / 0.57 s |
| expr | g++ | 0.34 s / 0.47 s | 0.34 s / 0.47 s | 0.73 s / 0.51 s | 0.73 s / 0.51 s |
| words | clang++ | 0.29 s / 0.29 s | 0.30 s / 0.29 s | 0.53 s / 0.42 s | 0.53 s / 0.42 s |
| words | g++ | 0.31 s / 0.28 s | 0.30 s / 0.28 s | 0.66 s / 0.42 s | 0.66 s / 0.42 s |

Where Dazzle's build time goes, from `dz build --timings`:

| Program | Compiler | Build | Dazzle's own phases | C and C++ headers | C++ compiler and link | Other |
|---|---|---|---:|---:|---:|---:|
| astar | clang++ | debug, from clean | 99 ms | 2 ms | 227 ms | 10 ms |
| astar | clang++ | debug, one-line change | 97 ms | 2 ms | 230 ms | 10 ms |
| astar | clang++ | release, from clean | 97 ms | 2 ms | 433 ms | 10 ms |
| astar | clang++ | release, one-line change | 98 ms | 2 ms | 435 ms | 10 ms |
| astar | g++ | debug, from clean | 98 ms | 2 ms | 240 ms | 11 ms |
| astar | g++ | debug, one-line change | 99 ms | 2 ms | 241 ms | 9 ms |
| astar | g++ | release, from clean | 98 ms | 2 ms | 498 ms | 9 ms |
| astar | g++ | release, one-line change | 97 ms | 2 ms | 499 ms | 11 ms |
| expr | clang++ | debug, from clean | 92 ms | 2 ms | 219 ms | 8 ms |
| expr | clang++ | debug, one-line change | 90 ms | 2 ms | 222 ms | 9 ms |
| expr | clang++ | release, from clean | 91 ms | 2 ms | 549 ms | 9 ms |
| expr | clang++ | release, one-line change | 89 ms | 2 ms | 552 ms | 8 ms |
| expr | g++ | debug, from clean | 90 ms | 2 ms | 242 ms | 9 ms |
| expr | g++ | debug, one-line change | 89 ms | 2 ms | 239 ms | 7 ms |
| expr | g++ | release, from clean | 90 ms | 2 ms | 628 ms | 9 ms |
| expr | g++ | release, one-line change | 91 ms | 2 ms | 629 ms | 8 ms |
| words | clang++ | debug, from clean | 89 ms | 2 ms | 189 ms | 8 ms |
| words | clang++ | debug, one-line change | 90 ms | 2 ms | 195 ms | 9 ms |
| words | clang++ | release, from clean | 89 ms | 2 ms | 428 ms | 8 ms |
| words | clang++ | release, one-line change | 90 ms | 2 ms | 433 ms | 7 ms |
| words | g++ | debug, from clean | 89 ms | 2 ms | 206 ms | 10 ms |
| words | g++ | debug, one-line change | 90 ms | 2 ms | 205 ms | 8 ms |
| words | g++ | release, from clean | 91 ms | 2 ms | 559 ms | 8 ms |
| words | g++ | release, one-line change | 89 ms | 2 ms | 558 ms | 8 ms |

Run time is the median of 10 runs of each half, alternating. Build time is the median of 3 builds, in debug and in `--release`. From clean, `build/` is deleted first: for Dazzle, `dz build` then compiles the program, and finds the runtime and the standard library already compiled, and `stdlib.h` already imported, in its per-user cache, as a C++ program finds its standard library already built; for C++, one compiler invocation. After a one-line change, a function is appended to the program's source and its number changed before each build, one more build first not counted: Dazzle compiles the program's own generated C++ again and links, and C++ compiles its one file again. Dazzle's own phases are reading the manifests, parsing, macros, resolving names, type checking, ownership, placement, instantiating generics and emitting the C++. Other is what no phase accounts for, mostly starting `dz` and linking the vendored libraries. Peak memory is the largest resident set of any run.

- CPU: AMD Ryzen 7 H 255 w/ Radeon 780M Graphics (16 threads)
- OS: Linux-6.12.115+deb13.1-amd64-x86_64-with-glibc2.41
- `clang++`: Debian clang version 19.1.7 (3+b1)
- `g++`: g++ (Debian 14.2.0-19) 14.2.0
- Mode: `--release`. C++ flags: `-std=c++17 -fno-exceptions -fno-rtti -Wall -Wextra -O2 -flto=auto`; Dazzle's generated code gets the same and `-fwrapv -fno-finite-loops -fno-strict-aliasing`.
- Dazzle: commit f398a8b plus uncommitted changes to the compiler or the programs
- Load average before / after: 1.91 / 1.28
