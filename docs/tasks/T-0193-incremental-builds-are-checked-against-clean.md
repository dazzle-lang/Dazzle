---
id: T-0193
title: Incremental builds are checked against clean ones: an edit corpus
milestone: M15
status: done
kind: feature
area: tooling
---

The safety net for the incremental build (T-0192's build stamp, then per-module reuse): an
edit must never leave a build different from one made from clean. Each scenario builds a
project, applies one scripted edit, builds again in place, then builds a copy of the edited
sources from clean, and requires the same `build/cpp` byte for byte (each folder's own path
replaced) and the same program output.

Edits: a leaf and a root body, the root's signature, a module added and removed, a
cross-module constant, default argument, field default, `Drop` body and reflected type (on a
generated 10-module `bench_scale` project), a generic body, a library's field default and a
reflected field (on `tests/run/generic_methods`, `dynamic` and `reflect`). It passes against
today's compiler, which re-analyses everything; each later step must keep it passing. Every
build sets `DZ_INCREMENTAL` itself (`verify` incremental, `0` clean) for when the compiler
reads it.

It found that a removed module's generated files stay in `build/cpp` (T-0194).

## Where the code is

| What | Where |
|---|---|
| the scenarios and the check | `crates/dz/tests/incremental.rs` |
| the generated project | `tools/bench_scale.py generate` |
| the test map row | `docs/testing.md` |
