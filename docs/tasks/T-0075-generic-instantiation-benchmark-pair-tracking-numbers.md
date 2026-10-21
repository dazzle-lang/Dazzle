---
id: T-0075
title: A generic-instantiation benchmark pair, and tracking the numbers
milestone: M15
status: open
kind: feature
area: bench
---

**What is missing.** Two things, and the second is the one that matters.

**A generic-instantiation pair in `tests/bench`.** M15's benchmarks compare Dazzle against
hand-written C++ doing the same work, and generics are the case with the most to prove:
Dazzle monomorphizes on its own side (§8.4) rather than handing templates to C++, which is
what makes error messages point at user code (invariant 5, principle 4). The question that
deserves a number is whether that costs anything at run time — it should not, since the
output is concrete C++ either way — and what it costs at *compile* time, where
per-instance emission with include guards is doing work a C++ template would do differently.
So the pair is: a generic container and algorithm instantiated several times in Dazzle,
against the equivalent C++ templates, measured for both run time and build time.

**Somewhere to track the numbers over time.** `dz bench` reports a run; nothing keeps it.
**A single run is a snapshot, not a baseline** — it says Dazzle is within some percentage of
C++ today and says nothing about whether yesterday's change made it worse. Principle 6 is
"compile fast" and principle 1 is about control; neither is defended by a number nobody
compares against.

**Why it matters.** A performance regression is the kind that passes every test. The
corpus, the feature matrix and the error corpus all catch behaviour changes; nothing catches
a change that makes the generated C++ 20% slower or the build twice as long. That is the
gap, and it is wider than a missing benchmark: it is a missing *ratchet*.

**What it would take.**

1. The benchmark pair itself, in `tests/bench` beside the existing ones.
2. **Machine-readable output** — which is
   [T-0066](T-0066-parseable-test-bench-output.md) (`dz bench --format=json`) — since nothing
   can be tracked that has to be read by eye first. These two should be done together.
3. A stored baseline, checked in, and a comparison that fails or warns past a threshold.
   The hard part is **noise**: CI runners vary enough that a naive threshold either misses
   real regressions or fires constantly. The usual answers are to compare ratios against the
   C++ pair measured in the same run (which this corpus is already shaped for — a pair, not
   an absolute) rather than against absolute times, and to keep the threshold loose enough
   to catch a change of kind rather than of degree.
4. Decide where the numbers live. A file in the repository is simplest and reviewable in a
   diff; it also makes every benchmark run a commit, which is its own friction.

Related: [T-0012](T-0012-measure-compile-time-at-scale.md) — compile time at scale, which is
the same missing-measurement problem for the compiler rather than for the generated code.
