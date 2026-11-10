---
id: T-0012
title: Measure compile time at scale
milestone: none
status: done
kind: feature
area: tooling
where: `tools/bench_scale.py`; results in `tests/bench_programs/SCALE.md`
---

**Done** (2026-11-09). `python tools/bench_scale.py` generates a project of N modules (about
510 lines of Dazzle each: structs with methods, `Eq`/`Ord` impls, a generic container over
each struct, an enum with a `switch`, a `HashMap`, calls into two earlier modules) and the
same program in C++, which it builds as `make -j` builds a plain project, with no
precompiled headers. It times four debug builds of each half: from clean, nothing changed,
a one-line change to the leaf module and to the root one, with `dz`'s phases apart from the
C++ compiler. Both halves must print the same checksum. Results, and the before and after of
the 0.2 build-time work, are in `tests/bench_programs/SCALE.md`.

At 10 modules Dazzle is about 2.5× C++ from clean and close after an edit. It does not
scale: at 100 modules (51 000 lines, clang++) a build from clean takes 60 s against 5.3 s, a
build with nothing changed 8.8 s against 0.05 s, and a one-line change to the leaf module
15 s against 1.1 s. Three causes, each its own task:

- [T-0185](T-0185-generic-instance-method-bodies-are-in.md): every method of every generic
  instance is in its module's header, so each module parses the instances of every module
  beneath it. The C++ compile grows with the square of the project.
- [T-0186](T-0186-checking-that-objects-are-up-date.md): the up-to-date check hashes every
  header each object includes, on every build: 3.2 s at 100 modules with nothing changed.
- [T-0187](T-0187-dazzle-own-phases-grow-faster-than.md): emitting, ownership, placement and
  instantiation nearly triple as the project doubles: 4.6 s of every build at 100 modules.

It also found that `#line` directives named sources as the project's path was spelled, so
building it by another spelling compiled every file again; they now name the full path.
Panic locations do the same, which is a question for the user
([T-0188](T-0188-panic-location-names-its-file-as.md)).

Before the 0.2 work (`f6b62d5`) every build of 10 modules took 9.7 s, edit or not; after
T-0102 and T-0181 it is 0.6–1.9 s. T-0182, T-0183 and T-0101 change little at this size,
where `std` is a small part of the build. Tracking over time is shared with T-0075.

**What is missing.** A **generated 20k–50k-line project**, built cold and warm through
`dz build`, with the numbers recorded. **Principle 6 is "compile fast" and nothing measures
it.**

Everything measured today is small: `tests/run/hello` analyses in around 61 ms in a debug
build, of which ~40 ms is type-checking `std`
([T-0025](T-0025-std-type-checked-in-every-analysis.md)). That tells us about the constant
and nothing about the slope.

**Why it matters.** The slope is where the surprises are, and the compiler has at least three
places where the cost is plausibly worse than linear, none of which shows at `hello`'s size:

- **Whole-program analysis.** Symbols, the type table and HIR are built for the whole program
  at once ([T-0023](T-0023-no-incremental-analysis.md)), so every file is re-analysed on
  every build and, in the LSP, on every keystroke.
- **Monomorphization.** One instance per set of type arguments, each behind its own include
  guard (§8.4). A project with a few generic containers over a few dozen types is a
  multiplication, and the generated C++ grows with it.
- **The C++ compile that follows.** `dz build` hands its output to a C++ compiler, so the
  real figure a user feels includes that — and generated code's shape (how much lands in
  headers, how many instances each translation unit includes) is something Dazzle controls
  and nothing currently checks.

A compiler that is fast at 1k lines and slow at 50k has a problem that is very expensive to
discover late, because by then the structure that causes it is the structure everything else
depends on.

**What it would take.**

1. **A generator**, not a checked-in 50k-line project: a script that emits N modules with a
   realistic shape — structs, impls, generic containers instantiated over several types,
   cross-module calls, a few macros — parameterised on N so the *slope* can be measured
   rather than one point. Checked in; its output is not.
2. **Cold and warm**, separately: cold is a fresh build directory, warm is one file changed.
   The two answer different questions, and the warm figure is the one a user feels all day.
3. **Per-phase timing**, so a regression names the phase. Invariant 3 — every phase a pure
   function of its input — makes that straightforward to instrument.
4. **Both halves of the build**: Dazzle's own time and the C++ compile, reported separately.
5. Tracked over time, which is the same ratchet
   [T-0075](T-0075-generic-instantiation-benchmark-pair-tracking-numbers.md) needs and should
   share.

It is unscheduled rather than deferred: nothing is blocked on it, and it is the kind of
measurement whose value is in having been taken *before* the structure is hard to change —
which argues for doing it sooner than its position in the backlog suggests.

**A first data point (2026-10-24, T-0090).** The whole-program benchmarks are three programs
of 250–400 lines each. Built from clean with `--release` on a Ryzen 7 H 255, `dz build`
takes 2–3.5 s, where the same program in C++ takes 0.3–0.6 s with the same compiler.
`dz cpp` alone is about 0.5 s; the rest is the C++ compiler on the generated code. That code
includes the standard library's C++, about 1 MB of headers, rebuilt on every build.
`python tools/bench_programs.py` reports both halves on every run
(`tests/bench_programs/RESULTS.md`).
