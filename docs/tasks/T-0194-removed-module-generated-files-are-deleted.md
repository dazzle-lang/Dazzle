---
id: T-0194
title: A removed module's generated files are deleted from build/cpp
milestone: M15
status: open
kind: feature
area: tooling
---

When a module leaves a project (its directory and its dependency line deleted), the next
`dz build` writes every remaining module's files but leaves the removed one's `<module>.h`,
`<module>.cpp` and `<module>.inst.cpp` in `build/cpp`. Nothing includes or compiles them, so
the program is right, but `build/cpp` is then not what a clean build makes. Found by T-0193's
`module_removed` scenario, which tolerates exactly those files until this is done; drop that
allowance (the `removed` filter in `verify`) in the same change.

It matters more once per-module reuse (stage 2 of the incremental build) keeps unaffected
modules' files on disk: that step has to tell a module removed from one left alone, and the
clean-up belongs in the same place. The emitter knows every file it writes (`emitted.files`),
so the fix is to delete any other `.h`/`.cpp` in `build/cpp` after writing, and the object
and stamp a deleted file had under `build/obj`.

This is the case [T-0103](T-0103-renamed-or-removed-module-generated-c.md) already
holds for renamed and removed modules, found again by a test; whichever change fixes it
closes both.

## Where the code is

| What | Where |
|---|---|
| writing `build/cpp` | `crates/dz/src/build.rs`, `emit_cpp` |
| the test that finds it | `crates/dz/tests/incremental.rs`, `module_removed` and `verify`'s `removed` |
