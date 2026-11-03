| Program | Compiler | Dazzle | C++ | Dazzle / C++ | Peak memory (Dazzle / C++) | Build: Dazzle / C++ |
|---|---|---:|---:|---:|---:|---:|
| astar | clang++ | 470 ms | 414 ms | 1.13× | 19.4 MiB / 12.7 MiB | 2.02 s / 0.45 s |
| astar | g++ | 439 ms | 398 ms | 1.10× | 19.4 MiB / 12.7 MiB | 1.51 s / 0.39 s |
| expr | clang++ | 277 ms | 135 ms | 2.06× | 140.9 MiB / 150.1 MiB | 2.35 s / 0.58 s |
| expr | g++ | 243 ms | 146 ms | 1.66× | 140.8 MiB / 150.5 MiB | 1.69 s / 0.51 s |
| words | clang++ | 232 ms | 156 ms | 1.48× | 40.9 MiB / 32.1 MiB | 2.00 s / 0.42 s |
| words | g++ | 233 ms | 168 ms | 1.39× | 42.4 MiB / 32.4 MiB | 1.51 s / 0.42 s |

Where Dazzle's build time goes, from `dz build --timings`:

| Program | Compiler | Dazzle's own phases | C and C++ headers | C++ compiler | Other |
|---|---|---:|---:|---:|---:|
| astar | clang++ | 95 ms | 83 ms | 1831 ms | 13 ms |
| astar | g++ | 94 ms | 84 ms | 1324 ms | 10 ms |
| expr | clang++ | 86 ms | 84 ms | 2164 ms | 11 ms |
| expr | g++ | 87 ms | 85 ms | 1509 ms | 10 ms |
| words | clang++ | 86 ms | 84 ms | 1820 ms | 11 ms |
| words | g++ | 85 ms | 84 ms | 1331 ms | 9 ms |

Run time is the median of 10 runs of each half, alternating. Build time is the median of 3 builds from clean: for Dazzle, `dz build`, which compiles the program and the standard library; for C++, one compiler invocation. Dazzle's own phases are reading the manifests, parsing, macros, resolving names, type checking, ownership, placement, instantiating generics and emitting the C++. Other is what no phase accounts for, mostly starting `dz` and linking the vendored libraries. Peak memory is the largest resident set of any run.

- CPU: AMD Ryzen 7 H 255 w/ Radeon 780M Graphics (16 threads)
- OS: Linux-6.12.115+deb13.1-amd64-x86_64-with-glibc2.41
- `clang++`: Debian clang version 19.1.7 (3+b1)
- `g++`: g++ (Debian 14.2.0-19) 14.2.0
- Mode: `--release`. C++ flags: `-std=c++17 -fno-exceptions -fno-rtti -Wall -Wextra -O2 -flto=auto`; Dazzle's generated code gets the same and `-fwrapv`.
- Dazzle: commit 4eaf644
- Load average before / after: 4.74 / 2.09
