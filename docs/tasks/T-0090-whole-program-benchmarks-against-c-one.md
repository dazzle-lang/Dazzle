---
id: T-0090
title: Whole-program benchmarks against C++: one real algorithm, written in both
milestone: M15
status: done
done: 2026-10-24
kind: feature
area: bench
---

**What is missing.** `tests/bench` compares Dazzle and C++ one construct at a time:
indexing, a RAII loop, a virtual call. Each pair is a few lines, and on the machines it has
run on the two sides are within a few percent. That proves the constructs compile to the
same thing; it does not say what a *program* costs. Nobody chooses a language by how fast a
single indexed load is. The comparison people ask for is a whole program: an algorithm that
uses allocation, collections, strings, generics, traits, errors and ownership together,
written idiomatically in each language, built at the same optimisation level and timed end
to end.

**Why it matters.** It is the number the website and README would show (the user asked for
it on 2026-10-24, when reviewing how Dazzle presents itself). A per-construct table invites
"of course one instruction is the same"; a whole program is where hidden costs would show
up if they existed: bounds checks in a hot loop, reset-to-defaults on every move, the
`Error` path, `String` and `List` against `std::string` and `std::vector`, monomorphized
generics against templates.

**Why it has not been built.** M15's goal was the harness and the pairing (same binary,
same flags, results asserted equal), and single constructs were the simplest way to prove
it. The harness is reusable as it is.

**What it would take.** Compiler work and a corpus, no language change.

1. Choose two or three programs that a C++ programmer would recognise as real work, not
   microbenchmarks. Candidates: a path-finder (A* on a grid, with a priority queue and a
   hash map), a tokenizer and evaluator for a small expression language (strings, tagged
   unions, errors), an n-body or particle simulation (structs of floats, tight loops), and a
   word-frequency count over a large generated text (hashing, sorting, strings).
2. Write each idiomatically on both sides: the C++ as a reviewer would accept it
   (`std::vector`, `std::unordered_map`, RAII, no hand-tuning the Dazzle side lacks), the
   Dazzle with the standard library. Vendor the C++ beside `tests/bench/vendor/baseline`, or
   as a second suite if timing a whole program wants its own `main`.
3. Keep the suite's rule that both halves compute the same result, and print the checksum.
4. Report run time, peak memory and build time, since "compile fast" is principle 6.

Related: [T-0075](T-0075-generic-instantiation-benchmark-pair-tracking-numbers.md), which
adds a generic pair and a way to track numbers over time, and
[T-0066](T-0066-parseable-test-bench-output.md), machine-readable `dz bench` output.

**Done (2026-10-24).** `tests/bench_programs` has three programs, each written as
`src/main.dz` and `main.cpp`: A* search (`astar`), a tokenizer, parser and evaluator
(`expr`), and a word count (`words`). `tools/bench_programs.py` builds both halves with each
compiler at the same optimisation level, runs them alternately, refuses to report a pair
whose checksums differ, and writes run time, peak memory and build time with the machine
and compilers to `RESULTS.md`. A test in `crates/dz/tests/run.rs` holds every pair to the
same checksum on a small input. With `Allocators::Temp` used where it helps (`expr`, `words`), Dazzle came out at 1.3–2.1× C++. The gaps it found
are [T-0096](T-0096-list-sort-by-heapsort-through-function.md) (sorting),
[T-0097](T-0097-moving-union-through-fallible-result-costs.md) (moving unions through
results) and [T-0098](T-0098-str-cannot-be-cut-str-b.md) (cutting a `str`), plus three
codegen bugs fixed at the time. `docs/log/tooling.md` has the details.
