---
id: T-0154
title: Link-time optimisation in release and shipping builds
milestone: M15
status: done
kind: feature
area: tooling
---

Every module is its own C++ translation unit, so an optimised build could not inline a
call from one module into another. Compiling the benchmarks' generated C++ with `-flto` by
hand (2026-11-03) made `astar` 17% and `words` 19% faster on clang++, and `words` 12% on
g++.

**Where the code is**

| Piece | Where |
|---|---|
| `-flto=auto` on the command that compiles and links Dazzle's code, for `--release` and `--shipping`; tried once without it, with a note, when the link fails | `build` in `crates/dz/src/build.rs` |
| The vendored libraries keep their recipes' flags (`opt` only) | `build_libraries` in `crates/dz/src/native.rs` |

**Done 2026-11-03**, with `#[inline]` (T-0153); T-0153 has the measurements. `=auto` runs
the link-time work in parallel, and keeps g++ from printing a note about running it
serially, which would break the warning-clean rule (spec §16.5). A toolchain whose linker
cannot read LTO objects (LLVM's clang targeting MSVC without `lld`) gets a plain build and
`note: built without link-time optimisation`; that path was tested with a stand-in
compiler that fails any link given `-flto=auto`, not on Windows itself.
