# Code generation, DZ08xx

These come from turning a checked program into C++ and building it (spec §16). The C++ is
generated from a program the checker accepted, so most of them are the compiler's fault, and
say so.

## DZ0801

The checker accepted something the C++ generator has no translation for yet. The help names
the task that tracks it in `docs/tasks/`. Until it lands, write the construct another way.

No example: which constructs these are changes as the generator catches up.

## DZ0802

A program to run needs `fn main()`, taking nothing and returning nothing — or returning
`i32`, the process's exit code. A project whose `[build] output` is a library needs none.

No example: `dz check` does not ask for an entry point; only a build does.

## DZ0803

Either no C++ compiler was found — install clang++ or g++, or name one with `--cxx` or
`DZ_CXX` — or the generated C++ failed to compile, which is always a bug in Dazzle
(invariant 7). The C++ is kept in `build/cpp` for a bug report.

No example: it depends on the machine, or on a compiler bug.

## DZ0804

A later phase met something an earlier one should have reported or never made. It is a bug
in the Dazzle compiler, reported as one rather than surfacing as bad C++.

No example: a program that produces it is a compiler bug to report.
