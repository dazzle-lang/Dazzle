| Program | Compiler | Dazzle | C++ | Dazzle / C++ | Peak memory (Dazzle / C++) | Build: Dazzle / C++ |
|---|---|---:|---:|---:|---:|---:|
| astar | clang++ | 422 ms | 488 ms | 0.87× | 19.4 MiB / 12.6 MiB | 2.32 s / 0.54 s |
| astar | g++ | 479 ms | 465 ms | 1.03× | 19.5 MiB / 12.6 MiB | 1.71 s / 0.46 s |
| expr | clang++ | 216 ms | 162 ms | 1.33× | 140.7 MiB / 150.5 MiB | 2.61 s / 0.68 s |
| expr | g++ | 204 ms | 170 ms | 1.20× | 140.8 MiB / 150.5 MiB | 2.08 s / 0.59 s |
| words | clang++ | 170 ms | 205 ms | 0.83× | 41.0 MiB / 32.2 MiB | 2.43 s / 0.50 s |
| words | g++ | 201 ms | 227 ms | 0.88× | 42.0 MiB / 32.0 MiB | 1.99 s / 0.52 s |

Where Dazzle's build time goes, from `dz build --timings`:

| Program | Compiler | Dazzle's own phases | C and C++ headers | C++ compiler | Other |
|---|---|---:|---:|---:|---:|
| astar | clang++ | 111 ms | 96 ms | 2099 ms | 14 ms |
| astar | g++ | 110 ms | 91 ms | 1494 ms | 13 ms |
| expr | clang++ | 104 ms | 96 ms | 2400 ms | 14 ms |
| expr | g++ | 103 ms | 98 ms | 1869 ms | 13 ms |
| words | clang++ | 101 ms | 95 ms | 2222 ms | 12 ms |
| words | g++ | 114 ms | 97 ms | 1770 ms | 11 ms |

Run time is the median of 9 runs of each half, alternating. Build time is the median of 1 builds from clean: for Dazzle, `dz build`, which compiles the program and the standard library; for C++, one compiler invocation. Dazzle's own phases are reading the manifests, parsing, macros, resolving names, type checking, ownership, placement, instantiating generics and emitting the C++. Other is what no phase accounts for, mostly starting `dz` and linking the vendored libraries. Peak memory is the largest resident set of any run.

- CPU: AMD Ryzen 7 H 255 w/ Radeon 780M Graphics (16 threads)
- OS: Linux-6.12.115+deb13.1-amd64-x86_64-with-glibc2.41
- `clang++`: Debian clang version 19.1.7 (3+b1)
- `g++`: g++ (Debian 14.2.0-19) 14.2.0
- Mode: `--release`. C++ flags: `-std=c++17 -fno-exceptions -fno-rtti -Wall -Wextra -O2 -flto=auto`; Dazzle's generated code gets the same and `-fwrapv`.
- Dazzle: commit 9dab5cd plus uncommitted changes to the compiler or the programs
- Load average before / after: 5.74 / 4.08
