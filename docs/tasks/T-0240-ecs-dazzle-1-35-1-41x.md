---
id: T-0240
title: "ecs: Dazzle is 1.35-1.41x its C++ half"
milestone: M15
status: done
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

**Progress, 2026-11-17: 1.14× and 1.09×.** Dazzle ran 1.33× C++'s instructions on clang++
and 1.45× on g++ (callgrind, 600 units for 20 steps), so the gap was work done, not
memory. Two causes, one per compiler, found by editing the generated C++ before it was
compiled (a wrapper standing in for the compiler) and counting again:

- **g++: a failing bounds check was not `[[noreturn]]`.** `List`'s `index` calls
  `list_index_out_of_bounds` when the index is past the end, a function that only panics,
  but its C++ declaration was plain `void`. After every check g++ had to assume the call
  returned having changed any memory, reloaded the list, and could not merge one check
  into the next. A function whose body can never return, with no `return` in it and a last
  statement that never completes, is now `[[noreturn]]` (`never_returns`,
  `noreturn_attr`): g++ 911 M → 786 M instructions, 161 → 132 ms. clang++ works it out
  itself under LTO and did not move; `astar` on g++ fell 1.8%, nothing else moved.
- **clang++: the query's `get` was not inlined.** clang++ priced it above its threshold,
  so every element was two calls, `get` and `seek`, with the iterator kept in memory;
  forced inline, clang++ ran 280 → 204 ms. `get` was big because it branched on which
  storage the query walks, three checked reads each way. The query now keeps what `seek`
  found, the entity and its row in each storage, so `get` is two reads with no branch, and
  `valid` compares with the walked storage's length, taken once, since nothing can change
  a storage while a query holds it. The same in the C++ half's iterator, as the rules ask;
  Rust's `query2` already worked out both rows in one closure. clang++ 280 → 212 ms.

`RESULTS.md` with both (load 1.1): Dazzle 214 ms on clang++ and 135 on g++, 1.14× and
1.09× C++'s 188 and 125 (the C++ half's own iterator change moved it 208 → 188 and
115 → 125). Against Rust (240, 240 and 112 ms), Dazzle on g++ is 0.56× the versions that
keep the nested queries and 1.21× the index loops.

**One cost:** `astar` on g++ is 5% slower with `[[noreturn]]` (327 → 345 ms, fifteen
interleaved runs), though it runs 1.8% fewer instructions: g++ lays its code out
differently, and marking the functions `cold` as well does not change it. Kept, for
`ecs`'s 18%; the other programs did not move.

**Tried and not kept:** the bounds check's failure path `cold` as well (no change); `List::get`
`#[inline]` and `row_of` written with it (more code, slower); `#[inline]` on the query's
methods (their bodies reached the header, and clang++ still declined: `seek` costs 410
against its 325).

**What is left** is the checks themselves: `row_of` checks `m_entities[row]` and `get`
checks both value rows, where C++ checks nothing. With the query as it was, forcing
`seek` and `get` inline gave clang++ 153 ms, faster than C++, but only by forcing it: `#[inline]` says the C++ compiler decides
(§12.8), and an attribute that forces it, Rust's `#[inline(always)]`, would be a language
change, the user's call. So would emitting an iterator's `valid`, `get` and `advance`
`always_inline` as a rule of codegen, and g++ refuses `always_inline` where it cannot
inline (recursion), so it is not free.

**Then, the user's choice: `#[inline]` inlines every call (r137, D-0107).** With the query's
and the storage's small methods and `World`'s lookups written `#[inline]`, `ecs` is 130 ms on
clang++ and 127 on g++ (`RESULTS.md`, load 1.1), 0.70× and 1.02× its C++ half, and 0.54× and
1.16× Rust's collecting and index-loop versions. Written `#[inline]` while the C++ compiler
still decided, the same methods changed nothing (212 and 134 ms).

| Piece | Where |
|---|---|
| `#[inline]` forced | `forced_inline`, `inline_keyword`, `emit_body` in `crates/dz_cpp/src/lib.rs` |
| `[[noreturn]]` | `never_returns`, `noreturn_attr`, `free_fn` in `crates/dz_cpp/src/lib.rs` |
| The query that keeps both rows | `Query2` in `tests/bench_programs/ecs/libs/ecs/src/query.dz`, `query2` in `world.dz`; the iterator in `ecs.hpp` |
| The test | `a_function_that_never_returns_is_noreturn` in `crates/dz/tests/run.rs` |
