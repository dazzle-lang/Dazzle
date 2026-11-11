---
id: T-0199
title: A clean build at scale parses every module's headers twice, and every List instance whole
milestone: M15
status: open
kind: feature
area: tooling
where: `compile` in `crates/dz/src/build.rs`; `dz_cpp::emit` and the instance class declarations in `crates/dz_cpp/src/lib.rs`
---

**Fix 1 done** (2026-11-11): when more files need compiling than there are cores, a
module's `.cpp` and `.inst.cpp` are compiled as one, `build/obj/dz/<module>.both.cpp`,
which includes both. From clean at 100 modules: 13.19 s to 8.99 s (C++: 5.22 s), the C++
compiler 11.28 s to 7.11 s; 50 modules 5.10 s to 3.83 s. Edits are as they were (a leaf
edit 1.08 s to 0.99 s). The first body edit after a clean build compiles the instance file
on its own once, in parallel with the module's own precompiled header (T-0195), which takes
longer. Left: fixes 2 and 3 below.

## Where the code is

| What | Where |
|---|---|
| Which pairs are compiled as one, and when | `combine` in `crates/dz/src/build.rs`, called from `compile_and_link` |
| A module's header precompiled after a combined build | `Together::before`, in `compile_and_link`'s `editing` loop |
| The two files written, per module | `dz_cpp::emit` in `crates/dz_cpp/src/lib.rs` |
| The instance class declarations, and their bodies (fix 2) | `Emitter::instance_decl` and `Emitter::instance_struct_bodies` in `crates/dz_cpp/src/lib.rs` |
| Tested by | the incremental corpus, `crates/dz/tests/incremental.rs`: its 10-module first build has more files than a 16-core machine, so it combines, and each edit then compiles apart |

**What is wrong.** After T-0198 an edit at 100 modules costs what `make` does, but a build
from clean is still 2.5× C++'s: 13.2 s against 5.2 s (`tests/bench_programs/SCALE.md`),
of which 11.3 s is the C++ compiler. Profiled on 2026-11-11 (clang++ 19, debug,
`target/scale/100` from `tools/bench_scale.py`, each file timed alone on an idle machine):

- **It is parsing, not code.** Dazzle writes 51 MB of C++ for a program that is 1.1 MB by
  hand: 40 MB of `.inst.cpp`, 6 MB of module `.cpp`, 4.9 MB of module headers (49 KB each,
  against 4 KB in C++). `#line` is 56% of an instance file's bytes but costs nothing
  measurable (0.43 s with it, 0.44 s without). `m050.h` alone parses in 0.24 s and
  `m099.h` in 0.46 s; the pch for `std` takes 0.04 s.
- **Every `List` instance is a class parsed in full.** clang's `-ftime-trace` of `m050.cpp`:
  167 ms of `ParseClass`, of which 110 ms is the 306 `List<PartN>` instances its headers
  bring in, each with ~45 member declarations, `sort_by`, `heapsort`, `retain` and the rest,
  whatever the program calls. C++ parses `std::vector` once as a template (4 ms) and
  instantiates what it uses. A rough cut that drops half of them (by method name, program-wide)
  made `m050.cpp` 0.30 s → 0.25 s and `m099.h` 0.46 s → 0.37 s.
- **Each module's headers are parsed twice.** T-0191 put a module's instance bodies in a
  `.inst.cpp` of their own, so a body edit does not compile them again; it noted ~45% more
  CPU from clean. Measured: `m050.cpp` 0.30 s + `m050.inst.cpp` 0.44 s, against 0.48 s
  for one file including both; `m099`, 0.53 s + 0.67 s against 0.71 s. A third to two
  fifths of a module's compile time from clean.
- **Instance files compile methods nobody calls** (T-0190): `m050.inst.cpp` defines every
  `List` method for all six of its structs, 0.14 s of code generation past its parse.

**What it would take**, cheapest first:

1. **One translation unit per module when both its files need compiling**: a file that
   `#include`s the module's `.cpp` and `.inst.cpp`, giving one object. That is a build from
   clean and an edit to a module's interface. A body edit compiles the `.cpp` alone, so the
   instance file needs an object of its own then; the first body edit after a clean build
   compiles it too, in parallel (0.44 s against 0.30 s for that edit, once). Expected:
   about a third less C++ CPU from clean. Build-side only, no emitted C++ changes.
2. **Declare only the instance methods the program calls**, and define only those (and
   what their bodies call). Program-wide, not per module as T-0190 planned, so a method
   missed is a C++ compile error in every build rather than a link error in some. Special
   members (constructors, destructor, copy, move) always stay, since C++ calls them
   without the emitter writing it. The cost: a first call to an instance method changes
   the header of the module that owns the instance, and the modules above it are redone.
   This would also do what is left of T-0190.
3. **Emit a generic struct's layout as a C++ class template** in `std.h`, with the
   bodies still monomorphized by Dazzle as explicit specializations. That reopens D-0014,
   whose condition for reopening ("compiler-side mono ever dominates build times") is not
   what was found, so it is the user's call and last.

1 and 2 together could bring a clean build near C++'s (a module's file about 0.35–0.4 s
against 0.34 s). Compiler work only, for `docs/log/`.
