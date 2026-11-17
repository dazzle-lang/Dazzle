---
id: T-0075
title: A generic-instantiation benchmark pair, and tracking the numbers
milestone: M15
status: done
kind: feature
area: bench
where: `tests/bench_programs/generics/` (the pair); `tools/bench_history.py` (records, the comparison, `HISTORY.md`, the backfill); `tools/bench_programs.py` (`--record`, `--check`, the comparison after a run); `tests/bench_programs/history.jsonl`, `HISTORY.md`
---

**Done 2026-11-17.**

- **The pair: `tests/bench_programs/generics`.** One generic driver run for six key types,
  `i64`, `u32`, a `Cell` struct, `String`, `Pair<i32, i32>` and `Pair<String, u16>`, each
  with its own value type: an AVL tree map of the program's own in one `List`, bounded by
  `Ord` and two traits of the program's (`Digest`, `Tally`); a quicksort, a binary search
  and a count of distinct elements; a `Pair<A, B>` whose `Eq`, `Ord` and `Digest` are
  generic impls; keys and values made through function values. The C++ half is the same
  code as templates, written out rather than `std::map` and `std::sort`, since the question
  is the program's own generic code and not two libraries. Both halves print the same
  checksum on both compilers, and the suite's `whole_program_benchmarks_agree_with_their_cpp_halves`
  holds them to it.
- **What it says** (`RESULTS.md`, 2026-11-17, at r140): at run time Dazzle's
  monomorphization costs nothing, 0.85× on clang++ and 0.93× on g++ at the same peak
  memory. At build time from clean it does: 1.27 s against 0.89 on clang++ and 1.17 against
  0.91 on g++ in release, 0.58 against 0.38 and 0.72 against 0.47 in debug. Dazzle's own
  phases are 112 ms of it; the rest is the C++ compiler compiling the 12,000 lines of
  instances Dazzle emitted (`std`'s `List` of each node and key type among them), where the
  C++ half is 375 lines of templates. After a one-line change Dazzle is ahead, 0.21 s
  against 0.38 in debug and 0.65 against 0.88 in release.
- **The numbers over time: `tests/bench_programs/history.jsonl`**, one line per program and
  compiler per run recorded, with both halves' times, peak memory, the build from clean and
  after a one-line change, and Dazzle's own phases; `HISTORY.md` beside it is written from
  it, each ratio run after run, one table per CPU and compiler version. The history starts
  with every version of `RESULTS.md` git held, 26 commits from 2026-10-24, read back by
  `python tools/bench_history.py backfill` (216 records), so it is a record from the first
  run, not from today; it already shows a step nobody had written down, `json` on g++
  from 1.05× to 1.00× with r137.
- **The ratchet.** Every full run of `tools/bench_programs.py` ends by comparing itself
  with the last run recorded on the same CPU and compiler version, ratio against ratio, and
  marks a ratio **worse** past 10% for run time and 20% for a build from clean; `--check`
  makes that exit 1, `--record` adds the run. Ratios rather than times, since the halves of
  one run share the machine (the first run compared moved by 0–1% on every program but one
  build, +4%). Where the numbers live, item 4 below: a file in the repository, recorded by
  hand when a run is published in `RESULTS.md`, not by CI, whose shared runners are too
  noisy for even a ratio and would make every run a commit.
- **Machine-readable output** was already there, `bench_programs.py --json`; the history is
  built from it. [T-0066](T-0066-parseable-test-bench-output.md), `dz test` and `dz bench`
  with `--format=json`, is a separate matter and stays open.
- **A compiler bug found on the way**: `&a < b` (and `==`, `cmp`) with `b` a `&T`, `T` a
  number, emitted the left `&` as a pointer, `compare(addr(a), b)`, which no C++ compiler
  took; only a right-hand `&` was dropped. Both are now (`native_trait_call` in
  `crates/dz_cpp/src/lib.rs`), pinned by `tests/run/generics`.

What follows is the task as it was written.

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
