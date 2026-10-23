---
id: T-0087
title: A build compiles C++ left behind by a module that no longer exists
milestone: none
status: open
kind: bug
area: tooling
---

**What is wrong.** `dz build` writes one `.cpp` and `.h` per module into `build/cpp/`, then
compiles every `.cpp` in that folder. A file whose module has since gone stays there and is
compiled anyway. `examples/ownership` once depended on a module `books`, which the public
release removed. A checkout that had built it before still had `build/cpp/books.cpp`, and
`every_example_builds` failed with `DZ0803`. The C++ errors pointed at `::dz::alloc`, which
r74 removed. Nothing in the program was wrong, and deleting the stale files fixed it
(2026-10-23).

**Why it matters.** A failure that depends on what a checkout built before is hard to read.
The message says the compiler has a bug, and the source it names no longer exists. It will hit
anyone who renames or removes a module.

**What it would take.** Either compile only the files this build wrote (the driver knows the
list), or delete files in `build/cpp/` that this build did not write. The first is safer: it
never deletes anything a user put there. Compiler work only, for `docs/log/`.
