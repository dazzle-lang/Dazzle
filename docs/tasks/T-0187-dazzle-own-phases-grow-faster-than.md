---
id: T-0187
title: Dazzle's own phases grow faster than the program: emitting, placement, ownership, instantiation
milestone: M15
status: open
kind: feature
area: tooling
where: `crates/dz_cpp` (emit), `crates/dz_flow` (ownership, placement), `crates/dz_mono`
---

**What is wrong.** T-0012's generated project (`tools/bench_scale.py`), a build with nothing
changed, clang++, release `dz`. Milliseconds per phase as the project doubles:

| Phase | 10 modules | 25 | 50 | 100 |
|---|---:|---:|---:|---:|
| emit C++ | 114 | 317 | 583 | 1 576 |
| ownership | 86 | 209 | 474 | 1 396 |
| placement | 45 | 142 | 352 | 1 104 |
| instantiate generics | 32 | 98 | 206 | 589 |
| type check | 24 | 50 | 81 | 211 |
| lex and parse | 12 | 20 | 27 | 52 |

Parsing grows with the program; the four at the top nearly triple each time it doubles. At
100 modules (51 000 lines) Dazzle's own phases are 4.6 s of every build, an edit or not.

**What it would take.** Profile the 100-module project to find what in each phase is not
linear: a scan of every module or every instance inside a loop over them is the usual
shape. Separate from T-0023 (re-analysing only what changed), which would hide the slope
for an edit but not for a build from clean. Compiler work, for `docs/log/`.

