| Program | Compiler | Dazzle | C++ | Dazzle / C++ | Peak memory (Dazzle / C++) | Build: Dazzle / C++ |
|---|---|---:|---:|---:|---:|---:|
| astar | clang++ | 400 ms | 495 ms | 0.81× | 17.5 MiB / 12.6 MiB | 2.37 s / 0.53 s |
| astar | g++ | 449 ms | 471 ms | 0.95× | 17.4 MiB / 12.6 MiB | 1.75 s / 0.45 s |
| expr | clang++ | 189 ms | 161 ms | 1.17× | 140.5 MiB / 150.5 MiB | 2.43 s / 0.68 s |
| expr | g++ | 186 ms | 170 ms | 1.10× | 140.9 MiB / 150.6 MiB | 1.98 s / 0.60 s |
| words | clang++ | 164 ms | 210 ms | 0.78× | 40.4 MiB / 32.0 MiB | 2.39 s / 0.49 s |
| words | g++ | 194 ms | 223 ms | 0.87× | 40.2 MiB / 32.4 MiB | 1.81 s / 0.48 s |

Where Dazzle's build time goes, from `dz build --timings`:

| Program | Compiler | Dazzle's own phases | C and C++ headers | C++ compiler | Other |
|---|---|---:|---:|---:|---:|
| astar | clang++ | 117 ms | 95 ms | 2144 ms | 12 ms |
| astar | g++ | 112 ms | 92 ms | 1535 ms | 13 ms |
| expr | clang++ | 103 ms | 92 ms | 2219 ms | 13 ms |
| expr | g++ | 104 ms | 94 ms | 1770 ms | 12 ms |
| words | clang++ | 104 ms | 93 ms | 2179 ms | 12 ms |
| words | g++ | 101 ms | 91 ms | 1609 ms | 12 ms |

Run time is the median of 9 runs of each half, alternating. Build time is the median of 1 builds from clean: for Dazzle, `dz build`, which compiles the program and the standard library; for C++, one compiler invocation. Dazzle's own phases are reading the manifests, parsing, macros, resolving names, type checking, ownership, placement, instantiating generics and emitting the C++. Other is what no phase accounts for, mostly starting `dz` and linking the vendored libraries. Peak memory is the largest resident set of any run.

- CPU: AMD Ryzen 7 H 255 w/ Radeon 780M Graphics (16 threads)
- OS: Linux-6.12.115+deb13.1-amd64-x86_64-with-glibc2.41
- `clang++`: Debian clang version 19.1.7 (3+b1)
- `g++`: g++ (Debian 14.2.0-19) 14.2.0
- Mode: `--release`. C++ flags: `-std=c++17 -fno-exceptions -fno-rtti -Wall -Wextra -O2 -flto=auto`; Dazzle's generated code gets the same and `-fwrapv`.
- Dazzle: commit ce8fa8a plus uncommitted changes to the compiler or the programs
- Load average before / after: 25.18 / 12.67
