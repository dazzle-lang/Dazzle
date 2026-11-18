---
id: T-0255
title: What -fwrapv, -fno-finite-loops and -fno-strict-aliasing promise, under cl.exe
milestone: M21
status: open
kind: design
area: codegen
where: `crates/dz/src/build.rs` (~464–480, the comments say what each flag is for); `crates/dz_cpp/src/lib.rs` (~6088, emission relying on `-fwrapv`)
---

Part of M21, and the one piece of it that is not mechanical: three of the flags `dz` passes
are not style but meaning, and `cl.exe` has none of them.

- **`-fwrapv`**: plain `+`, `-` and `*` wrap on overflow (18.63). `cl.exe` has no such
  promise; it rarely exploits signed overflow today, but rarely is not never. The portable
  answer is to emit signed arithmetic through the unsigned type (`(int32_t)((uint32_t)a +
  (uint32_t)b)`), which every optimiser turns back into one instruction. Either for every
  compiler, which drops a flag and a dependency on it, or only for MSVC; measure first.
- **`-fno-finite-loops`**: a loop with no side effects is run as written, not assumed to
  end. Check what MSVC's optimiser assumes; a `volatile` access in such a loop is the
  fallback.
- **`-fno-strict-aliasing`**: MSVC does no type-based alias analysis, so nothing is
  needed; say so in a comment and a test.

Decide each with the user before T-0253 lands, since the first may change generated code
on every compiler.
