| Program | Compiler | Dazzle | C++ | Dazzle / C++ | Peak memory (Dazzle / C++) | Build: Dazzle / C++ |
|---|---|---:|---:|---:|---:|---:|
| astar | clang++ | 386 ms | 418 ms | 0.92× | 19.4 MiB / 12.7 MiB | 2.02 s / 0.47 s |
| astar | g++ | 431 ms | 398 ms | 1.08× | 19.4 MiB / 12.6 MiB | 1.51 s / 0.39 s |
| expr | clang++ | 199 ms | 135 ms | 1.47× | 140.9 MiB / 150.5 MiB | 2.42 s / 0.66 s |
| expr | g++ | 223 ms | 183 ms | 1.22× | 140.8 MiB / 150.7 MiB | 1.79 s / 0.53 s |
| words | clang++ | 179 ms | 232 ms | 0.77× | 41.5 MiB / 31.9 MiB | 2.44 s / 0.50 s |
| words | g++ | 162 ms | 167 ms | 0.97× | 41.0 MiB / 32.2 MiB | 1.80 s / 0.44 s |

Where Dazzle's build time goes, from `dz build --timings`:

| Program | Compiler | Dazzle's own phases | C and C++ headers | C++ compiler | Other |
|---|---|---:|---:|---:|---:|
| astar | clang++ | 101 ms | 83 ms | 1823 ms | 11 ms |
| astar | g++ | 100 ms | 83 ms | 1320 ms | 11 ms |
| expr | clang++ | 91 ms | 84 ms | 2233 ms | 11 ms |
| expr | g++ | 92 ms | 84 ms | 1603 ms | 10 ms |
| words | clang++ | 106 ms | 94 ms | 2233 ms | 10 ms |
| words | g++ | 109 ms | 94 ms | 1582 ms | 11 ms |

Run time is the median of 9 runs of each half, alternating. Build time is the median of 1 builds from clean: for Dazzle, `dz build`, which compiles the program and the standard library; for C++, one compiler invocation. Dazzle's own phases are reading the manifests, parsing, macros, resolving names, type checking, ownership, placement, instantiating generics and emitting the C++. Other is what no phase accounts for, mostly starting `dz` and linking the vendored libraries. Peak memory is the largest resident set of any run.

- CPU: AMD Ryzen 7 H 255 w/ Radeon 780M Graphics (16 threads)
- OS: Linux-6.12.115+deb13.1-amd64-x86_64-with-glibc2.41
- `clang++`: Debian clang version 19.1.7 (3+b1)
- `g++`: g++ (Debian 14.2.0-19) 14.2.0
- Mode: `--release`. C++ flags: `-std=c++17 -fno-exceptions -fno-rtti -Wall -Wextra -O2 -flto=auto`; Dazzle's generated code gets the same and `-fwrapv`.
- Dazzle: commit 920ce0b plus uncommitted changes to the compiler or the programs
- Load average before / after: 13.0 / 7.93
