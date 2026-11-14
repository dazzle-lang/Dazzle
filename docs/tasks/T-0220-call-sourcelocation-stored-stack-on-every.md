---
id: T-0220
title: A call's SourceLocation is stored to the stack on every pass, though only a cold path reads it
milestone: M15
status: open
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

**Where the code is.**

| Piece | Where |
|---|---|
| `#location` as a default argument, rendered at the call | `render_default`, `source_location` in `crates/dz_cpp/src/lib.rs` |
| `SourceLocation`, `panic_bounds`, `index` | `crates/dz_cpp/src/runtime.rs` |
| `List::push`, `make_room` | `crates/dz_project/src/std/collections/list.dz` |
