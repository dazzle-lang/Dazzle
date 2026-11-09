---
id: T-0174
title: A raytrace benchmark: floating-point and vector math
milestone: M15
status: open
kind: feature
area: bench
---

Planned for 0.2 by the user on 2026-11-09, one of two new whole-program benchmarks.

**What it is.** `tests/bench_programs/raytrace`: the ray tracer in `examples/stb`, without the
PNG, rendering a fixed scene at a fixed size and samples per pixel, its random numbers from
the shared SplitMix64. The Dazzle half uses `std::math`'s `Vec3`; the C++ half a `Vec3`
struct with operators, as C++ code would. Each prints a `checksum` over the image.

**Why it matters.** The three programs there now never touch floating point. Games are
mostly float and vector math: operator overloading through traits, small structs passed by
value, `sqrt` and friends. This is where the generated C++ has to be as good as hand-written
C++, and nothing measures it.

**The thing to get right: both halves must compute the same floats.** The same operations in
the same order give the same bits, but:

- clang contracts `a * b + c` into a fused multiply-add by default and g++ does not in ISO
  mode, and Dazzle's generated code may split an expression into statements where the C++
  half does not, which changes what can be contracted. Build both halves with
  `-ffp-contract=off`, or show that the checksums agree without it.
- The checksum is over the image quantised to bytes, as the PNG would be. If a few pixels
  still differ by one, the check allows a small count of them and says so, rather than
  hiding a real difference behind a loose tolerance.

**What it would take.** `src/main.dz` (starting from `examples/stb/src/main.dz`), `main.cpp`,
`dazzle.toml`, `quick_args.txt`, a row in the README's table and `RESULTS.md`, and any flag
added for floats in `tools/bench_programs.py` for both halves alike. A benchmark; no language
change.
