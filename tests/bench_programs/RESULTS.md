| Program | Compiler | Dazzle | C++ | Dazzle / C++ | Peak memory (Dazzle / C++) | Build: Dazzle / C++ |
|---|---|---:|---:|---:|---:|---:|
| astar | clang++ | 422 ms | 501 ms | 0.84× | 19.4 MiB / 12.7 MiB | 2.30 s / 0.53 s |
| astar | g++ | 479 ms | 506 ms | 0.95× | 19.4 MiB / 12.7 MiB | 1.72 s / 0.48 s |
| expr | clang++ | 222 ms | 159 ms | 1.40× | 141.0 MiB / 150.5 MiB | 2.54 s / 0.68 s |
| expr | g++ | 216 ms | 170 ms | 1.27× | 140.8 MiB / 150.6 MiB | 2.04 s / 0.58 s |
| words | clang++ | 237 ms | 214 ms | 1.11× | 41.3 MiB / 31.9 MiB | 2.30 s / 0.49 s |
| words | g++ | 177 ms | 166 ms | 1.07× | 40.9 MiB / 32.4 MiB | 1.71 s / 0.42 s |

Where Dazzle's build time goes, from `dz build --timings`:

| Program | Compiler | Dazzle's own phases | C and C++ headers | C++ compiler | Other |
|---|---|---:|---:|---:|---:|
| astar | clang++ | 108 ms | 95 ms | 2084 ms | 11 ms |
| astar | g++ | 110 ms | 93 ms | 1506 ms | 13 ms |
| expr | clang++ | 99 ms | 91 ms | 2334 ms | 13 ms |
| expr | g++ | 104 ms | 91 ms | 1833 ms | 12 ms |
| words | clang++ | 99 ms | 94 ms | 2100 ms | 10 ms |
| words | g++ | 99 ms | 93 ms | 1509 ms | 11 ms |

Run time is the median of 9 runs of each half, alternating. Build time is the median of 1 builds from clean: for Dazzle, `dz build`, which compiles the program and the standard library; for C++, one compiler invocation. Dazzle's own phases are reading the manifests, parsing, macros, resolving names, type checking, ownership, placement, instantiating generics and emitting the C++. Other is what no phase accounts for, mostly starting `dz` and linking the vendored libraries. Peak memory is the largest resident set of any run.

- CPU: AMD Ryzen 7 H 255 w/ Radeon 780M Graphics (16 threads)
- OS: Linux-6.12.115+deb13.1-amd64-x86_64-with-glibc2.41
- `clang++`: Debian clang version 19.1.7 (3+b1)
- `g++`: g++ (Debian 14.2.0-19) 14.2.0
- Mode: `--release`. C++ flags: `-std=c++17 -fno-exceptions -fno-rtti -Wall -Wextra -O2 -flto=auto`; Dazzle's generated code gets the same and `-fwrapv`.
- Dazzle: commit c145a3f plus uncommitted changes to the compiler or the programs
- Load average before / after: 10.89 / 6.34
