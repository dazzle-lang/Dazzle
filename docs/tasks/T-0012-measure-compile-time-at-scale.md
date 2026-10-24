---
id: T-0012
title: Measure compile time at scale
milestone: none
status: open
kind: feature
area: tooling
---

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
