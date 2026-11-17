---
id: T-0240
title: "ecs: Dazzle is 1.35-1.41x its C++ half"
milestone: M15
status: open
kind: feature
area: codegen
---

Found 2026-11-17, the first run of `ecs` (T-0172), `tests/bench_programs/RESULTS.md`:

| `ecs` | Dazzle | C++ | Rust `collect` | Rust `disjoint` | Rust `cell` |
|---|---:|---:|---:|---:|---:|
| clang++ | 281 ms | 208 ms | 243 ms | 113 ms | 243 ms |
| g++ | 161 ms | 115 ms | | | |

The other programs are 0.79–1.06× their C++ halves. `ecs` is the first to index arrays in
tight loops, which is where a bounds check could still cost something: almost all its time
is `separate` and `fight`, two loops over 600 units nested in two more, each inner step a
query probing a sparse set (`row_of`: `m_sparse[index]`, `m_entities[row]`) and reading
`m_values[row]`, every one checked, where C++ checks none. Rust checks them too, and its
`disjoint` version, plain index loops, runs as fast as C++ on g++.

**Measure first**, as T-0222 did: callgrind both halves on both compilers, and compare the
inner loop of `separate` instruction by instruction. Candidates: the checks themselves, the
query's iterator (a view struct whose `get` returns a tuple of references, through
`Iterate`), the `scan_first` branch inside `seek` and `get`, and `Entity`'s comparison.
Write the faster shape by hand in the generated C++ before changing the compiler.

| What | Where |
|---|---|
| The program | `tests/bench_programs/ecs/` (`libs/ecs/src/query.dz`, `storage.dz`; `src/main.dz`) |
| The C++ half it is measured against | `tests/bench_programs/ecs/ecs.hpp`, `main.cpp` |
| How indexing is emitted | `crates/dz_cpp/src/lib.rs` (`::dz::rt::index`), `crates/dz_cpp/src/runtime.rs` |
