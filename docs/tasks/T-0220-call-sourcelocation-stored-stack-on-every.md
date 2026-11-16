---
id: T-0220
title: A call's SourceLocation is stored to the stack on every pass, though only a cold path reads it
milestone: M15
status: done
kind: feature
area: codegen
---

Found 2026-11-14 reading the benchmarks' machine code (T-0215, T-0217).

**What is wrong.** A `SourceLocation` is three words (file, function, line and column),
passed by value. Where a call takes one, `List::push(v, loc: SourceLocation = #location)`
or the runtime's checked `::dz::rt::index(c, i, where)`, the compilers build it on the stack
on every pass of a loop, before the test that decides whether it is needed:

- **`List::push`**, both compilers: three stores of the location per element pushed, read
  only when the list must grow (`make_room`). Seen in the fill loop of a scratch benchmark
  of 100 000 `push`es (clang++ 19, `dz build --release`).
- **A checked index under g++ 14**: two stores per index, ahead of the compare, read only by
  `panic_bounds`. Every checked slice index pays it; clang++ moves them to the failing path.

Measured in neither case yet: the stores are cheap next to a cache miss, but they are in
the innermost loops a program has.

**What might fix it.** Pass a location as a pointer to one constant per call site, `static
constexpr SourceLocation` in the generated C++, rather than as a value: one register,
nothing stored. `SourceLocation` stays a value in Dazzle (spec §12.7); only how codegen
passes the hidden default changes. Measure `push` loops and indexed loops on both compilers
before and after; check that the precompiled `std` and the runtime shim (`panic_bounds`)
change together.

**Done 2026-11-16.** Each location a function passes is one `static constexpr` constant,
declared at the top of the function's body once it is known which the body uses, and the
runtime's checks (`index`, `div`, `rem`, the panics) take it by `const&`, so a check
passes the constant's address and stores nothing until it fails. A location outside a body,
in a field's default or a constant, is the temporary it was. `a_checks_location_is_a_constant_of_its_function`
in `dz/tests/run.rs` pins it, and that the panic still names the line.

Measured by instruction counts (callgrind), which a busy machine does not change, against
the compiler before (clang++ / g++):

| Program | Every location parameter `const&` | The runtime's checks only (built) |
|---|---:|---:|
| `astar` | -2.4% / -0.4% | -2.0% / +0.1% |
| `expr` | -3.6% / -5.3% | -3.6% / -4.2% |
| `json` | -0.4% / **+3.1%** | -1.8% / -3.4% (with T-0225's float text) |
| `words` | | -6.0% / -2.0% |
| `raytrace` | level | level |

**A Dazzle function's `loc: SourceLocation` stays a value**, copied from the constant. Taken
by `const&` as well, as the task first proposed, it stopped g++ inlining `String::push_str`
and `String::push` into `json`'s writer: 360 M instructions in calls that had been inline,
3.1% more in all. A parameter is never assigned in Dazzle, so the reference would have been
safe; it is only g++'s inliner that it upsets.

| Piece | Where |
|---|---|
| The constants, collected and declared | `source_location`, `site_consts`, `emit_body_here` in `crates/dz_cpp/src/lib.rs` |
| The checks taking a location by `const&` | `index`, `subslice`, `div`, `rem`, `panic*`, `unreachable`, `make_error`, `checked_error` in `crates/dz_cpp/src/runtime.rs` |
