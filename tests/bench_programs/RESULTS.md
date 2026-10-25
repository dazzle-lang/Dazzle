| Program | Compiler | Dazzle | C++ | Dazzle / C++ | Peak memory (Dazzle / C++) | Build: Dazzle / C++ |
|---|---|---:|---:|---:|---:|---:|
| astar | clang++ | 636 ms | 407 ms | 1.56× | 19.9 MiB / 13.2 MiB | 1.74 s / 0.30 s |
| astar | g++ | 505 ms | 398 ms | 1.27× | 19.9 MiB / 13.2 MiB | 2.55 s / 0.35 s |
| expr | clang++ | 323 ms | 138 ms | 2.34× | 141.3 MiB / 150.8 MiB | 1.80 s / 0.38 s |
| expr | g++ | 220 ms | 147 ms | 1.49× | 141.3 MiB / 150.8 MiB | 2.87 s / 0.47 s |
| words | clang++ | 295 ms | 170 ms | 1.74× | 70.8 MiB / 32.3 MiB | 1.74 s / 0.27 s |
| words | g++ | 295 ms | 208 ms | 1.42× | 70.8 MiB / 32.1 MiB | 2.52 s / 0.40 s |

Where Dazzle's build time goes, from `dz build --timings`:

| Program | Compiler | Dazzle's own phases | C and C++ headers | C++ compiler | Other |
|---|---|---:|---:|---:|---:|
| astar | clang++ | 38 ms | 379 ms | 1318 ms | 9 ms |
| astar | g++ | 38 ms | 379 ms | 2127 ms | 6 ms |
| expr | clang++ | 36 ms | 380 ms | 1369 ms | 14 ms |
| expr | g++ | 36 ms | 384 ms | 2440 ms | 13 ms |
| words | clang++ | 34 ms | 387 ms | 1312 ms | 7 ms |
| words | g++ | 35 ms | 377 ms | 2105 ms | 7 ms |

Run time is the median of 10 runs of each half, alternating. Build time is the median of 3 builds from clean: for Dazzle, `dz build`, which compiles the program and the standard library; for C++, one compiler invocation. Dazzle's own phases are reading the manifests, parsing, macros, resolving names, type checking, ownership, instantiating generics and emitting the C++. Other is what no phase accounts for, mostly starting `dz` and linking the vendored libraries. Peak memory is the largest resident set of any run.

- CPU: AMD Ryzen 7 H 255 w/ Radeon 780M Graphics (16 threads)
- OS: Linux-7.3.0-5-cachyos-x86_64-with-glibc2.44
- `clang++`: clang version 23.1.3
- `g++`: g++ (GCC) 16.2.1 20260810
- Mode: `--release`. C++ flags: `-std=c++17 -fno-exceptions -fno-rtti -Wall -Wextra -O2`; Dazzle's generated code gets the same and `-fwrapv`.
- Dazzle: commit fdbd580 plus uncommitted changes to the compiler or the programs
- Load average before / after: 1.43 / 2.07
