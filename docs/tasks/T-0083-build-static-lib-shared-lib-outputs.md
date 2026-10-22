---
id: T-0083
title: Build static-lib and shared-lib outputs
milestone: none
status: open
kind: feature
area: codegen
---

`[build] output` accepts `"executable"`, `"static-lib"` and `"shared-lib"` (spec §3.1), and
`dz_project` parses all three into `OutputKind`, but `dz build` refuses anything but an
executable: *"only executables can be built or run yet"* (`crates/dz/src/build.rs`). The
README presents Dazzle as something a C++ codebase can adopt one module at a time, which
needs this.

1. A static-lib module builds an archive (`.a`, or `.lib` on Windows), and a shared-lib
   module a `.so`, `.dylib` or `.dll`, into `build/`, with a header C++ code can include.
2. An end-to-end test where a small C++ program links a Dazzle library and calls it,
   warning-clean under clang++ and g++ (invariant 7).
3. **A design question to settle first, with the user:** what a Dazzle library's public
   C++ surface is. Which declarations are exported, under what names, and whether a
   generic's instances or an `!->` function can cross the boundary. That is language
   design, not something to decide in the implementation.

When it lands, update the README's "Where it fits" bullet that says library outputs are
"declared in the manifest but not built yet". Raised on 2026-10-22 while writing that
section.
