| Program | Compiler | Dazzle | C++ | Dazzle / C++ | Peak memory (Dazzle / C++) | Build: Dazzle / C++ |
|---|---|---:|---:|---:|---:|---:|
| astar | clang++ | 494 ms | 414 ms | 1.19× | 19.4 MiB / 12.7 MiB | 2.02 s / 0.45 s |
| astar | g++ | 443 ms | 396 ms | 1.12× | 19.5 MiB / 12.6 MiB | 1.51 s / 0.39 s |
| expr | clang++ | 282 ms | 134 ms | 2.10× | 141.0 MiB / 150.6 MiB | 2.33 s / 0.57 s |
| expr | g++ | 244 ms | 147 ms | 1.66× | 141.0 MiB / 150.6 MiB | 1.70 s / 0.51 s |
| words | clang++ | 193 ms | 158 ms | 1.23× | 41.1 MiB / 32.2 MiB | 2.02 s / 0.42 s |
| words | g++ | 212 ms | 168 ms | 1.27× | 41.0 MiB / 32.4 MiB | 1.53 s / 0.42 s |

Where Dazzle's build time goes, from `dz build --timings`:

| Program | Compiler | Dazzle's own phases | C and C++ headers | C++ compiler | Other |
|---|---|---:|---:|---:|---:|
| astar | clang++ | 95 ms | 84 ms | 1826 ms | 10 ms |
| astar | g++ | 94 ms | 85 ms | 1317 ms | 10 ms |
| expr | clang++ | 86 ms | 84 ms | 2146 ms | 10 ms |
| expr | g++ | 86 ms | 84 ms | 1522 ms | 11 ms |
| words | clang++ | 86 ms | 85 ms | 1842 ms | 10 ms |
| words | g++ | 85 ms | 84 ms | 1350 ms | 10 ms |

Run time is the median of 10 runs of each half, alternating. Build time is the median of 3 builds from clean: for Dazzle, `dz build`, which compiles the program and the standard library; for C++, one compiler invocation. Dazzle's own phases are reading the manifests, parsing, macros, resolving names, type checking, ownership, placement, instantiating generics and emitting the C++. Other is what no phase accounts for, mostly starting `dz` and linking the vendored libraries. Peak memory is the largest resident set of any run.

- CPU: AMD Ryzen 7 H 255 w/ Radeon 780M Graphics (16 threads)
- OS: Linux-6.12.115+deb13.1-amd64-x86_64-with-glibc2.41
- `clang++`: Debian clang version 19.1.7 (3+b1)
- `g++`: g++ (Debian 14.2.0-19) 14.2.0
- Mode: `--release`. C++ flags: `-std=c++17 -fno-exceptions -fno-rtti -Wall -Wextra -O2 -flto=auto`; Dazzle's generated code gets the same and `-fwrapv`.
- Dazzle: commit aa58e02
- Load average before / after: 1.55 / 1.34
