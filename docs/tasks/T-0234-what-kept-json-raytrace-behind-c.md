---
id: T-0234
title: "What kept json and raytrace behind C++ was not safety: a parameter moved in place, g++'s unit-growth limit"
milestone: M15
status: done
kind: feature
area: codegen
where: `crates/dz_cpp/src/lib.rs`: `moved_out_params`, `take_ownership_of_params`; `crates/dz/src/build.rs`: the g++ `--param` flags; `crates/dz/tests/run.rs`: `a_parameter_moved_on_is_used_in_place`
---

The user asked on 2026-11-15 whether `json` and `raytrace` running 7% to 23% behind their
C++ halves is what being safe costs, to be accepted, or something to improve. Measured
that day, on the machine and compilers of `tests/bench_programs/RESULTS.md`: **it is not
safety, and it can be improved.** Two of the causes found are fixed here; the rest are
listed for T-0225 and T-0222.

## Safety costs nothing in these two programs

Each row is a whole run, ratio to the C++ half, from the generated C++ built by hand.

| Build | json clang++ | json g++ | raytrace clang++ | raytrace g++ |
|---|---:|---:|---:|---:|
| as `dz build --release` builds it | 1.07× | 1.23× | 1.07× | 1.07× |
| every panic unreachable, and `-fno-wrapv -ffinite-loops -fstrict-aliasing` | 1.07× | 1.24× | 1.07× | 1.07× |

`panic`, `panic_bounds` and `panic_len` were defined as `__builtin_unreachable()`, so the
C++ compiler deleted every bounds, division and assert test in front of them: no call to
`panic_bounds` was left in the binaries (26 before in `json`, 17 in `raytrace`), and the
times did not move. The other way round, the C++ halves given Dazzle's three flags ran in
the same time, and with libstdc++'s bounds checks as well (`-D_GLIBCXX_ASSERTIONS`) `json`'s
ran 5% *faster* on both compilers. That last number is the noise floor: an unrelated flag
moves the same C++ source by 5%, so a gap of a few percent says little by itself.

These two programs index little in tight loops, where a bounds check can keep a loop from
being vectorised; nothing here says the checks are free everywhere.

## Where `json`'s time goes

By phase, each half timed from inside (ms, Dazzle / C++); generating the text, the checksum
and teardown are level.

| Phase | clang++ | g++ |
|---|---:|---:|
| parse the text into a tree | 107 / 108 | 105 / 93 |
| the tree to `List<Record>` | 29 / 20 | 30 / 21 |
| write the records out | 38 / 26 | 47 / 26 |

Each cause was taken out by a patch to the generated C++ (or, for the first, to the
program) and the phases timed again:

| Cause | clang++ | g++ | |
|---|---:|---:|---|
| The writer goes through reflection (a type-table lookup, `starts_with("List<")` and a scan of field names for every value); the C++ half's `to_json` is written by hand. A writer written by hand in Dazzle closes it. | 10 ms | 13 ms | a property of the benchmark; the user's call |
| `List::push` moved its element three times: into the parameter, into the body's own binding, into the list. | 4–5 ms | 2–7 ms | **fixed here** |
| g++ left `String::push_str` and `push` as calls, 24 of them, 253 M instructions; its C++ half's `std::string::append` is inlined. | 0 | 12–17 ms | **fixed here** |
| A float's text: `std::to_chars` in scientific form, `atoi` on its exponent (44 M instructions in `strtol`), then laid out again in plain decimal. | 2.5 ms | 1.6 ms | open, T-0225 |
| `main`'s region: with `DZ_REGIONS=0` parsing is 4–7 ms faster, converting 2 ms slower, the peak 9 MiB lower. | 4 ms | 4 ms | open, T-0233 |
| Converting, what is left: about 4–5 ms in the bare loop (a 144-byte `Res<Record>` built for each record, and five destructors run on values already moved from) and about 3 ms in the tags lists, which take twice the page faults of the C++ half's (3,800 against 2,000), with regions on or off, for a reason not found. Found by switching fields off in both halves, not by a fix. | 7 ms | 8 ms | open, T-0231 |

With the first four taken out by hand `json` ran 0.99× its C++ half on clang++ and 1.08× on
g++, same checksum.

## What was built

- **A by-value parameter the body moves on is used where it is** (`moved_out_params`). The
  body's own binding is there so a parameter drops at the body's end under every ABI
  (`take_ownership_of_params`). A parameter that is named once, by a `move` every path
  reaches before any `return`, is an empty husk by then, so where destroying it empty runs
  no code of the program's (`drops_silently`) the body binds a reference to the parameter
  and nothing is moved. The C++ signature does not change, so it holds for any function,
  a trait's or a `virtual` one included, and for a copyable value. `List::push` now moves
  its element once from a temporary and twice from a `move(x)`. A value holding a `drop` of
  the program's own keeps its binding: that `drop` runs on the emptied parameter, and
  `tests/run/discarded_move` pins how often and when.
  The further step, taking the caller's value by `&&` as a constructor does (T-0097), was
  measured at 1–2 ms more on `json` and not taken: a copyable argument is passed as an
  lvalue where no copy was placed, which `&&` does not bind.
- **g++ builds an optimised program with `--param inline-unit-growth=100`.** g++ stops
  inlining when a unit has grown by 40%, but only a unit of more than 10,000 instructions
  (`large-unit-insns`), and under link-time optimisation the unit is the whole program. A
  small C++ program is under that size and is never limited; a Dazzle program always
  carries `std`, so it always is. At 100 the inliner takes every call it finds worth it:
  `json`'s binary is the same at 100, 300 and 1000. `--param max-inline-insns-single`,
  `early-inlining-insns` and `inline-min-speedup` changed nothing, and forcing
  `#[inline]` with `always_inline` gained less (12–14 ms against 17) and would have gone
  against §12.8, which leaves the choice to the C++ compiler.

`RESULTS.md`, the whole suite run after both: `json` 1.21× to 1.13× on g++ (257 to 233 ms)
and 1.08× to 1.06× on clang++; `astar`, `expr`, `words` and `raytrace` within a percent of
where they were; build times unchanged.

## What this says about the rest

The generated C++ is lowered a statement at a time and leans on the C++ optimiser to clear
up after it. Most of it is cleared; what is left is extra moves, stores and calls, and
`raytrace`'s gap is the same kind (T-0222's four functions written cleanly on the same
types ran level with C++ on g++). Next, in order of what was measured: the float text's
round trip (T-0225), whether `json` is meant to price reflection (the user's call),
moved-from values' destructors and the shape of `Res<T>` (T-0231), `main`'s region
(T-0233), and T-0222's struct literals and temporaries.

## Where the code is

| What | Where |
|---|---|
| Which parameters a body always moves out | `crates/dz_cpp/src/lib.rs`: `moved_out_params` (and `consumed_params`, a constructor's `&&`) |
| Where the binding is written, or the reference | `crates/dz_cpp/src/lib.rs`: `take_ownership_of_params` |
| Whose drop may be seen | `crates/dz_cpp/src/lib.rs`: `drops_silently` |
| g++'s inlining flags | `crates/dz/src/build.rs`, beside `max-inline-insns-auto` (T-0190) |
| The test | `crates/dz/tests/run.rs`: `a_parameter_moved_on_is_used_in_place`; `tests/run/discarded_move` for a `drop` that prints |
