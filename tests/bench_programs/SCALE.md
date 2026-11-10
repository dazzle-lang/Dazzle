# Build time at scale (T-0012)

Written by `python tools/bench_scale.py --markdown tests/bench_programs/SCALE.md`, which generates a project of N modules and the same program in C++ (`target/scale/<N>/`).

| Modules | Lines (Dazzle / C++) | Compiler | from clean | nothing changed | leaf module changed | root module changed |
|---:|---|---|---:|---:|---:|---:|
| 10 | 5112 / 3440 | clang++ | 1.57 s / 0.59 s | 0.50 s / 0.00 s | 1.33 s / 0.40 s | 0.73 s / 0.39 s |
| 10 | 5112 / 3440 | g++ | 1.53 s / 0.67 s | 0.59 s / 0.00 s | 1.90 s / 0.73 s | 1.13 s / 0.72 s |
| 25 | 12777 / 8600 | clang++ | 6.81 s / 1.58 s | 1.34 s / 0.01 s | 3.46 s / 0.59 s | 1.64 s / 0.59 s |
| 25 | 12777 / 8600 | g++ | 5.92 s / 1.48 s | 1.16 s / 0.00 s | 3.01 s / 0.54 s | 1.43 s / 0.53 s |
| 50 | 25552 / 17200 | clang++ | 18.22 s / 2.61 s | 2.89 s / 0.02 s | 6.43 s / 0.69 s | 3.10 s / 0.64 s |
| 50 | 25552 / 17200 | g++ | 18.36 s / 2.89 s | 2.78 s / 0.02 s | 6.56 s / 0.73 s | 3.06 s / 0.70 s |
| 100 | 51102 / 34400 | clang++ | 60.01 s / 5.26 s | 8.80 s / 0.05 s | 15.16 s / 1.10 s | 8.28 s / 0.99 s |
| 100 | 51102 / 34400 | g++ | 66.75 s / 6.63 s | 8.68 s / 0.05 s | 19.55 s / 1.40 s | 9.26 s / 1.33 s |

Where Dazzle's time goes: its own phases / the C++ compiler and link.

| Modules | Compiler | from clean | nothing changed | leaf module changed | root module changed |
|---:|---|---:|---:|---:|---:|
| 10 | clang++ | 0.32 s / 1.24 s | 0.32 s / 0.16 s | 0.32 s / 0.99 s | 0.32 s / 0.39 s |
| 10 | g++ | 0.32 s / 1.20 s | 0.39 s / 0.16 s | 0.45 s / 1.41 s | 0.46 s / 0.62 s |
| 25 | clang++ | 0.85 s / 5.91 s | 0.85 s / 0.45 s | 0.85 s / 2.57 s | 0.87 s / 0.72 s |
| 25 | g++ | 0.77 s / 5.11 s | 0.78 s / 0.34 s | 0.76 s / 2.20 s | 0.76 s / 0.62 s |
| 50 | clang++ | 1.75 s / 16.39 s | 1.74 s / 1.06 s | 1.73 s / 4.62 s | 1.73 s / 1.29 s |
| 50 | g++ | 1.77 s / 16.53 s | 1.74 s / 0.97 s | 1.74 s / 4.74 s | 1.74 s / 1.25 s |
| 100 | clang++ | 4.65 s / 55.24 s | 4.98 s / 3.65 s | 4.54 s / 10.49 s | 4.53 s / 3.61 s |
| 100 | g++ | 4.69 s / 59.77 s | 5.02 s / 3.52 s | 5.19 s / 13.92 s | 5.15 s / 3.95 s |

Each cell is Dazzle / C++, the median of the builds asked for, in debug (`-O0 -g`). Dazzle is `dz build`. C++ is the same program as one header and one source per module, built as `make -j` builds a plain project: each source compiled on its own when it or a header it includes is newer than its object, one per core at once, with no precompiled headers, then linked. From clean deletes both `build/` folders (Dazzle's per-user cache stays, as C++'s standard library stays built). A changed module has one function's body changed, which changes no header: the leaf is the last module, which no other calls, and the root is module 0, which every other depends on. Dazzle's own phases are everything `--timings` reports but the headers, the libraries, the C++ compiler and the link.

- CPU: AMD Ryzen 7 H 255 w/ Radeon 780M Graphics (16 threads)
- OS: Linux-6.12.115+deb13.1-amd64-x86_64-with-glibc2.41
- `clang++`: Debian clang version 19.1.7 (3+b1)
- `g++`: g++ (Debian 14.2.0-19) 14.2.0
- `dz`: commit 1490e44
- Load average before / after: 0.17 / 4.19

## Before and after the 0.2 build-time work

clang++, one build per scenario, Dazzle / C++. `f6b62d5` is before T-0102 (every build compiled all the generated C++ in one command); `f398a8b` is after T-0102 and T-0181; `1490e44` is after T-0182, T-0183 and T-0101.

| Modules | dz | from clean | nothing changed | leaf module changed | root module changed |
|---:|---|---:|---:|---:|---:|
| 10 | f6b62d5 | 9.74 s / 0.63 s | 9.66 s / 0.00 s | 9.64 s / 0.40 s | 9.65 s / 0.39 s |
| 10 | f398a8b | 1.86 s / 0.58 s | 0.59 s / 0.00 s | 1.42 s / 0.40 s | 0.82 s / 0.39 s |
| 10 | 1490e44 | 1.57 s / 0.59 s | 0.50 s / 0.00 s | 1.33 s / 0.40 s | 0.73 s / 0.39 s |
| 25 | f398a8b | 6.28 s / 1.29 s | 1.32 s / 0.00 s | 3.16 s / 0.50 s | 1.55 s / 0.48 s |
| 25 | 1490e44 | 6.81 s / 1.58 s | 1.34 s / 0.01 s | 3.46 s / 0.59 s | 1.64 s / 0.59 s |
| 50 | f398a8b | 18.64 s / 2.60 s | 3.07 s / 0.01 s | 6.63 s / 0.71 s | 3.30 s / 0.65 s |
| 50 | 1490e44 | 18.22 s / 2.61 s | 2.89 s / 0.02 s | 6.43 s / 0.69 s | 3.10 s / 0.64 s |
