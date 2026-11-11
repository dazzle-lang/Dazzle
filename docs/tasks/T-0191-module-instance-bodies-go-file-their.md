---
id: T-0191
title: A module's instance bodies go to a file of their own, so a body edit does not compile them again
milestone: M15
status: done
kind: feature
area: tooling
---

Stage 3a of the incremental-build plan. Since T-0185 the first module to need a generic
instance defines its bodies, in that module's `.cpp`. In `bench_scale.py`'s 100-module
project that is 7 000 of the leaf `m099.cpp`'s 7 929 lines, and an edit to any body of the
module compiled them all again: about 0.45 s of a leaf edit's 1.06 s of C++ on clang++, and
1.3 s of 1.84 s on g++.

Now they are `<module>.inst.cpp`, an object of its own, and the module's `.cpp` keeps only
its own code. Their temporaries are numbered from zero, apart from the module's own, so an
edit that adds a temporary to the module's code does not rename theirs. A body edit leaves
the file byte for byte as it was, and the object cache (T-0102, T-0186) skips it. A module
that is the first to need no instance has no such file, rather than an empty one to compile
and link. `std`'s own instances stay with `std`, in its shared `std.inst.cpp`.

The cost is a build from clean: each instance file is one more translation unit, and it
parses its module's header again. At 100 modules (every one has instances) that is 101 more
files and about 45% more CPU time from clean (~105 → ~152 CPU-seconds on clang++).

## Where the code is

| What | Where |
|---|---|
| The two files written, per module | `dz_cpp::emit` in `crates/dz_cpp/src/lib.rs` (`instances`, after `body`) |
| Which module defines an instance body | `Emitter::place_instance_body`, `defined` |
| The temporaries numbered from zero | `Emitter::instance_definitions` |
| What the build compiles (every emitted `.cpp`) | `compile` in `crates/dz/src/build.rs` (`sources`) |
| The test | `a_body_edit_leaves_the_instance_file_unchanged` in `crates/dz/tests/run.rs` |
