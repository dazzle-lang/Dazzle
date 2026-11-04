| Program | Compiler | Dazzle | C++ | Dazzle / C++ | Peak memory (Dazzle / C++) | Build: Dazzle / C++ |
|---|---|---:|---:|---:|---:|---:|
| astar | clang++ | 367 ms | 415 ms | 0.89× | 17.4 MiB / 12.6 MiB | 1.98 s / 0.45 s |
| astar | g++ | 405 ms | 397 ms | 1.02× | 17.4 MiB / 12.7 MiB | 1.51 s / 0.38 s |
| expr | clang++ | 170 ms | 135 ms | 1.26× | 140.8 MiB / 150.6 MiB | 2.10 s / 0.57 s |
| expr | g++ | 164 ms | 147 ms | 1.11× | 140.7 MiB / 150.1 MiB | 1.66 s / 0.50 s |
| words | clang++ | 138 ms | 155 ms | 0.89× | 37.9 MiB / 32.3 MiB | 1.96 s / 0.42 s |
| words | g++ | 163 ms | 167 ms | 0.97× | 37.8 MiB / 32.4 MiB | 1.56 s / 0.42 s |

Where Dazzle's build time goes, from `dz build --timings`:

| Program | Compiler | Dazzle's own phases | C and C++ headers | C++ compiler | Other |
|---|---|---:|---:|---:|---:|
| astar | clang++ | 100 ms | 85 ms | 1787 ms | 11 ms |
| astar | g++ | 99 ms | 86 ms | 1312 ms | 12 ms |
| expr | clang++ | 92 ms | 85 ms | 1913 ms | 10 ms |
| expr | g++ | 91 ms | 84 ms | 1472 ms | 11 ms |
| words | clang++ | 90 ms | 82 ms | 1773 ms | 10 ms |
| words | g++ | 89 ms | 82 ms | 1381 ms | 10 ms |

Run time is the median of 9 runs of each half, alternating. Build time is the median of 1 builds from clean: for Dazzle, `dz build`, which compiles the program and the standard library; for C++, one compiler invocation. Dazzle's own phases are reading the manifests, parsing, macros, resolving names, type checking, ownership, placement, instantiating generics and emitting the C++. Other is what no phase accounts for, mostly starting `dz` and linking the vendored libraries. Peak memory is the largest resident set of any run.

- CPU: AMD Ryzen 7 H 255 w/ Radeon 780M Graphics (16 threads)
- OS: Linux-6.12.115+deb13.1-amd64-x86_64-with-glibc2.41
- `clang++`: Debian clang version 19.1.7 (3+b1)
- `g++`: g++ (Debian 14.2.0-19) 14.2.0
- Mode: `--release`. C++ flags: `-std=c++17 -fno-exceptions -fno-rtti -Wall -Wextra -O2 -flto=auto`; Dazzle's generated code gets the same and `-fwrapv`.
- Dazzle: commit b9bb60c plus uncommitted changes to the compiler or the programs
- Load average before / after: 3.71 / 2.44
