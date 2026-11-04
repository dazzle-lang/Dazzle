| Program | Compiler | Dazzle | C++ | Dazzle / C++ | Peak memory (Dazzle / C++) | Build: Dazzle / C++ |
|---|---|---:|---:|---:|---:|---:|
| astar | clang++ | 319 ms | 413 ms | 0.77× | 9.5 MiB / 12.6 MiB | 2.03 s / 0.45 s |
| astar | g++ | 350 ms | 397 ms | 0.88× | 9.4 MiB / 12.6 MiB | 1.57 s / 0.39 s |
| expr | clang++ | 157 ms | 134 ms | 1.17× | 141.0 MiB / 150.5 MiB | 2.11 s / 0.57 s |
| expr | g++ | 157 ms | 147 ms | 1.07× | 140.7 MiB / 150.1 MiB | 1.67 s / 0.50 s |
| words | clang++ | 141 ms | 155 ms | 0.91× | 37.6 MiB / 32.2 MiB | 1.99 s / 0.42 s |
| words | g++ | 171 ms | 166 ms | 1.02× | 37.7 MiB / 32.2 MiB | 1.60 s / 0.42 s |

Where Dazzle's build time goes, from `dz build --timings`:

| Program | Compiler | Dazzle's own phases | C and C++ headers | C++ compiler | Other |
|---|---|---:|---:|---:|---:|
| astar | clang++ | 99 ms | 83 ms | 1839 ms | 12 ms |
| astar | g++ | 99 ms | 85 ms | 1379 ms | 9 ms |
| expr | clang++ | 90 ms | 83 ms | 1923 ms | 11 ms |
| expr | g++ | 91 ms | 83 ms | 1483 ms | 11 ms |
| words | clang++ | 89 ms | 84 ms | 1808 ms | 10 ms |
| words | g++ | 90 ms | 84 ms | 1412 ms | 10 ms |

Run time is the median of 10 runs of each half, alternating. Build time is the median of 3 builds from clean: for Dazzle, `dz build`, which compiles the program and the standard library; for C++, one compiler invocation. Dazzle's own phases are reading the manifests, parsing, macros, resolving names, type checking, ownership, placement, instantiating generics and emitting the C++. Other is what no phase accounts for, mostly starting `dz` and linking the vendored libraries. Peak memory is the largest resident set of any run.

- CPU: AMD Ryzen 7 H 255 w/ Radeon 780M Graphics (16 threads)
- OS: Linux-6.12.115+deb13.1-amd64-x86_64-with-glibc2.41
- `clang++`: Debian clang version 19.1.7 (3+b1)
- `g++`: g++ (Debian 14.2.0-19) 14.2.0
- Mode: `--release`. C++ flags: `-std=c++17 -fno-exceptions -fno-rtti -Wall -Wextra -O2 -flto=auto`; Dazzle's generated code gets the same and `-fwrapv`.
- Dazzle: commit f0a0ab9
- Load average before / after: 3.26 / 1.81
