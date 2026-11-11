---
id: T-0190
title: Each module defines the instance methods it calls, as C++ instantiates a template's members
milestone: M15
status: open
kind: feature
area: codegen
where: `crates/dz_cpp/src/lib.rs`: `place_instance_body`, `instance_definitions`; `crates/dz_mono`
---

**Mostly done by T-0199** (2026-11-11): an instance outside `std` now declares and defines
only the methods the program calls, found from the whole program per generic struct
(`dz_mono::Called`). A method nobody calls is no longer compiled, which was this task's
first cost. What is left is the per-module half below, each module defining `inline` what
it calls, which only g++'s inlining would gain from and the inline budget already covers.

**Part done** (2026-11-09): g++'s inlining. An optimised build with g++ now passes
`--param max-inline-insns-auto=70`, the budget g++ gives a function declared `inline`, to
Dazzle's own code (`is_gcc` in `crates/dz/src/build.rs`, by what `--version` says). All three
benchmark programs run faster than they did before T-0185 or after it: `astar` 330 ms (352
before T-0185, 399 after), `expr` 151 ms (160), `words` 155 ms (165), against 396, 146 and
167 ms for C++. A release build takes 0.1-0.17 s longer. clang++ was not slower and warns at
the flag, so it does not get it.

**What is left** is the other half, compiling only the methods something calls, worth
0.03-0.06 s of a small program's debug rebuild. It is harder than the plan below makes it
look, which is why it was not done with the first half. Many of the C++ uses of an instance's
members are implicit, and the emitter never writes them: module B holds module A's struct,
which has a `List<Item>` field, by value, so B's file calls `List<Item>`'s destructor without
B ever naming `List<Item>`; copies, moves and assignments do the same, and so does the body of
an `#[inline]` function from another module's header. A file that uses a member `inline`
must define it, so each of those uses has to be found; one missed is a link error, and only
in a build where the compiler inlined every other call. T-0185's single owner, an ordinary
definition every file can link to, is what makes those safe today. A plan would have to
keep it as the fallback, or derive the implicit uses from the types each file holds by
value.

**What it is.** T-0185 moved generic instances' method bodies out of headers: each is now
defined once, in the file of the first module that needs it, as an ordinary function. That
took a 100-module project from 60 s to 12 s from clean. It cost two things, measured on the
single-file benchmarks:

- **Unused methods are compiled.** An `inline` method nobody calls was parsed but never
  compiled; an ordinary definition always is. So a small program's own file compiles every
  method of every instance it owns: a debug rebuild of `astar` went from 0.35 s to 0.41 s.
- **g++ inlines less.** At -O2, g++ inlines a function not declared `inline` only up to 15
  instructions, against 70 for one that is, and weighs that a body which must stay is less
  worth inlining. `astar` with g++ went from 352 ms to 396 ms. clang++ is unchanged, and so
  are `words` and `expr`. Raising g++'s budget (`--param max-inline-insns-auto=70`) fixes it, 329 ms; it first
  exposed T-0189, which is fixed, so that is now an option, though one that changes how
  g++ treats all of Dazzle's code rather than only the instances.

**What it would take.** Do what C++ does with a template's members: each module's file
defines, `inline`, exactly the instance methods it calls, and the ones those call, found
while emitting it. A method nobody calls is never written; the one file that uses a method
gets it `inline` as before; and headers stay as small as T-0185 made them. The care is in
what a header's `#[inline]` bodies call: a module that calls one must define what that body
calls too, or the link can miss a definition no file emitted. Compiler work, for
`docs/log/`.

