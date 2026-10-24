| Program | Compiler | Dazzle | C++ | Dazzle / C++ | Peak memory (Dazzle / C++) | Build: Dazzle (front end) / C++ |
|---|---|---:|---:|---:|---:|---:|
| astar | clang++ | 645 ms | 415 ms | 1.55× | 19.9 MiB / 13.2 MiB | 1.83 s (0.45 s) / 0.32 s |
| astar | g++ | 510 ms | 399 ms | 1.28× | 19.9 MiB / 13.2 MiB | 2.76 s (0.47 s) / 0.36 s |
| expr | clang++ | 347 ms | 164 ms | 2.12× | 139.6 MiB / 150.6 MiB | 1.85 s (0.44 s) / 0.41 s |
| expr | g++ | 239 ms | 168 ms | 1.43× | 139.7 MiB / 150.8 MiB | 2.95 s (0.44 s) / 0.49 s |
| words | clang++ | 302 ms | 174 ms | 1.74× | 70.8 MiB / 32.2 MiB | 1.81 s (0.62 s) / 0.30 s |
| words | g++ | 293 ms | 215 ms | 1.36× | 70.7 MiB / 32.2 MiB | 2.64 s (0.45 s) / 0.40 s |

Run time is the median of 10 runs of each half, alternating. Build time is from clean: for Dazzle, `dz build` (the front end, then the C++ compiler on the generated code and the standard library), with the front end alone, `dz cpp`, in brackets; for C++, one compiler invocation. Peak memory is the largest resident set of any run.

- CPU: AMD Ryzen 7 H 255 w/ Radeon 780M Graphics (16 threads)
- OS: Linux-7.3.0-4-cachyos-x86_64-with-glibc2.44
- `clang++`: clang version 23.1.3
- `g++`: g++ (GCC) 16.2.1 20260810
- Mode: `--release`. C++ flags: `-std=c++17 -fno-exceptions -fno-rtti -Wall -Wextra -O2`; Dazzle's generated code gets the same and `-fwrapv`.
- Dazzle: commit 63f583c plus uncommitted changes to the compiler or the programs
- Load average before / after: 1.85 / 2.0
