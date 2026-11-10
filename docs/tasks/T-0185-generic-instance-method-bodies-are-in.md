---
id: T-0185
title: A generic instance's method bodies are in its module's header, parsed again by every module above it
milestone: M15
status: done
kind: feature
area: tooling
where: `crates/dz_cpp/src/lib.rs`: `place_instance_body`, `defined`, `cpp_chunks`; the four instance paths that call it
---

**Done** (2026-11-09), the second way. An instance's struct and its methods' declarations stay
in the header of each module that needs it. Its bodies are defined once in the program, as
ordinary functions, in the file of the first module emitted that needs it; the prelude is
module 0, so `std`'s own instances stay in `std.cpp`, the same for every program. Bodies
written `#[inline]`, and associated constants, still go in every needing module's header,
behind a guard. `place_instance_body` does this for all four kinds of instance (a type's
members, a generic function, a generic impl's methods, a generic method), with a set of
defined guards shared across the program and per-namespace chunks for each file.

A module's header in the 100-module project went from 10 600 lines to 1 270, and `std.h`
to 2 400. At 100 modules (clang++, debug): from clean 60 s to 12.2 s (C++: 5.2 s), the leaf
module edited 15.2 s to 6.0 s (C++: 1.1 s), and with T-0186 nothing changed 8.8 s to 4.8 s.
The C++ work after an edit is now 1.0–1.3 s against C++'s 1.0–1.1 s; what is left is
Dazzle's own phases (T-0187).

It costs the single-file benchmarks a little: every method of an owned instance is now
compiled whether called or not (a debug rebuild of `astar` 0.35 s to 0.41 s), and g++
inlines an ordinary function less than an `inline` one (`astar` with g++ 352 ms to 396 ms;
clang++, `words` and `expr` unchanged). Both are [T-0190](T-0190-each-module-defines-instance-methods-it.md).
Raising g++'s inline budget instead found [T-0189](T-0189-words-computes-wrong-checksum-or-crashes.md).
Marking the bodies `inline __attribute__((used))` did not help g++, and clang++ then emitted
only one of a destructor's two symbols.

| Piece | Where |
|---|---|
| Where a body goes | `place_instance_body` in `crates/dz_cpp/src/lib.rs` |
| Which file defines it | `defined`, shared by every module's `Emitter` |
| Its namespace in the file | `cpp_chunks`, written after the module's own code in `emit` |
| Measured by | `tools/bench_scale.py`, `tests/bench_programs/SCALE.md` |

**What is wrong.** T-0012's generated project (`tools/bench_scale.py`) found it. A module of
about 500 lines of Dazzle, with six structs, gets a C++ header of 10 600 lines. 8 100 of them
are its six `List<Part>` instances: each instance's `_DEF` block is every method `List` has,
about 1 100 lines, with bodies, whether the module calls them or not. A header includes the
headers of the modules it depends on, so a module's `.cpp` parses the instances of every
module beneath it. In the generated project each module depends on the one before it, so
module 99 parses about a million lines to compile 900 of its own.

That makes a build from clean grow with the square of the project: 60 s for 100 modules
(51 000 lines) on clang++, against 5.3 s for the same program in C++ built as `make -j`
builds it, with no precompiled headers. A one-line change to the last module takes 15 s,
against 1.1 s, and 10.5 s of that is compiling that one file.

**Why C++ does not pay it.** A template's member functions are instantiated only where they
are called, and only in the translation unit that calls them; the header holds the template
once. Dazzle instantiates every member of every instance, up front, in the header of the
module that needs the instance (spec §8.4).

**What it would take.** Two ways, which can be combined:

1. Emit only the methods an instance's users reach, as C++ does. `dz_mono` already knows
   which instances each module needs; it would need to know which of their methods.
2. Keep an instance's struct and its methods' declarations in the header, and put the
   bodies in the `.cpp` of each module that calls them, as `inline` functions so two
   modules may both hold one. A module that includes the header then parses declarations,
   about 70 lines an instance rather than 1 100. Release builds lose nothing, since LTO
   inlines across modules (T-0154).

Measure with `python tools/bench_scale.py` before and after. Compiler work, for `docs/log/`.

