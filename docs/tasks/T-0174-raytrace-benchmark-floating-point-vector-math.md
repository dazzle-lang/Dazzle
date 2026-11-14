---
id: T-0174
title: A raytrace benchmark: floating-point and vector math
milestone: M15
status: done
kind: feature
area: bench
where: `tests/bench_programs/raytrace/` (`src/main.dz`, `main.cpp`, `cxx_flags.txt`, `quick_args.txt`); `tools/bench_programs.py`: `program_flags`; `crates/dz/tests/run.rs`: `whole_program_benchmarks_agree_with_their_cpp_halves`
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

**Done 2026-11-14.** Both halves render 960×540 at 16 rays a pixel and agree on every byte of
the image, with clang++ and g++. Both are built with `-ffp-contract=off`, from the program's
`cxx_flags.txt`, which `tools/bench_programs.py` and the test suite give both halves (the
Dazzle half through `DZ_CXXFLAGS`): on x86-64 without `-march` it changes nothing, and built
for `x86-64-v3` both halves fused alike and still agreed, but Dazzle splits some expressions
into statements where C++ does not, so agreement there was luck, not design. No pixel needed
a tolerance. The script now also builds each Dazzle half once uncounted before the clean
builds it times, so a program's own flags do not time compiling `std`.

First run (`RESULTS.md`): 1.25× its C++ half on clang++ (565 ms against 454) and 1.11× on
g++ (690 against 623); peak memory even; release builds 0.36 s and 0.46 s against 0.34 and
0.30. The switch cases borrow their payloads (`case Sphere(&s)`), as the C++ half's
`std::get_if` does; copying them cost 22 ms on clang++. Most of what was left on clang++ was
`intersect` not being inlined, since a private function had external linkage: fixed the
same day by T-0221, after which it was 1.10× on both compilers, and 1.07× after T-0222's
two fixes. The rest is T-0222.
