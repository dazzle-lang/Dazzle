---
id: T-0185
title: A generic instance's method bodies are in its module's header, parsed again by every module above it
milestone: M15
status: open
kind: feature
area: tooling
where: `crates/dz_cpp/src/lib.rs`: `instance_struct_bodies`, `instance_fn_body`, and where their `_DEF` blocks go
---

**What is wrong.** T-0012's generated project (`tools/bench_scale.py`) found it. A module of
about 500 lines of Dazzle, with six structs, gets a C++ header of 10 600 lines. 8 100 of them
are its six `List<Part>` instances: each instance's `_DEF` block is every method `List` has,
about 1 100 lines, with bodies, whether the module calls them or not. A header includes the
headers of the modules it depends on, so a module's `.cpp` parses the instances of every
module beneath it. In the generated project each module depends on the one before it, so
module 99 parses about a million lines to compile 900 of its own.

That makes a build from clean grow with the square of the project: 60 s for 100 modules
(51 000 lines) on clang++, against 5.3 s for the same program in C++ built as `make -j`
builds it, with no precompiled headers. A one-line change to the last module takes 15 s,
against 1.1 s, and 10.5 s of that is compiling that one file.

**Why C++ does not pay it.** A template's member functions are instantiated only where they
are called, and only in the translation unit that calls them; the header holds the template
once. Dazzle instantiates every member of every instance, up front, in the header of the
module that needs the instance (spec §8.4).

**What it would take.** Two ways, which can be combined:

1. Emit only the methods an instance's users reach, as C++ does. `dz_mono` already knows
   which instances each module needs; it would need to know which of their methods.
2. Keep an instance's struct and its methods' declarations in the header, and put the
   bodies in the `.cpp` of each module that calls them, as `inline` functions so two
   modules may both hold one. A module that includes the header then parses declarations,
   about 70 lines an instance rather than 1 100. Release builds lose nothing, since LTO
   inlines across modules (T-0154).

Measure with `python tools/bench_scale.py` before and after. Compiler work, for `docs/log/`.

