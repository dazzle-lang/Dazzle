---
id: T-0190
title: Each module defines the instance methods it calls, as C++ instantiates a template's members
milestone: M15
status: open
kind: feature
area: codegen
where: `crates/dz_cpp/src/lib.rs`: `place_instance_body`, `instance_definitions`; `crates/dz_mono`
---

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

