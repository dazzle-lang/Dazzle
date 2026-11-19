---
id: T-0255
title: What -fwrapv, -fno-finite-loops and -fno-strict-aliasing promise, under cl.exe
milestone: M21
status: open
kind: design
area: codegen
where: `crates/dz/src/build.rs` (~464–480, the comments say what each flag is for); `crates/dz_cpp/src/lib.rs` (`wrapping_unsigned`, `wrapping_op`, `as_unsigned` beside `int_bits`; their uses in `expr`'s `Unary` and `Binary` arms and `stmt`'s `Assign` arm; `expr_effects`, `block_effects`, `stmt_effects` beside `never_returns`, used in `stmt`'s `While` arm); `crates/dz_cpp/src/runtime.rs` (`DZ_LOOP_ALIVE`, `Wide<T>`, `wrapping_*_assign`); the test `arithmetic_wraps_and_empty_loops_are_kept_without_compiler_flags` in `crates/dz/tests/run.rs`
---

## Decided (2026-11-19)

Measured first, with `cl /O2` (MSVC 14.38) on a probe: it **folded `x * 2 / 2` to `x`**,
assuming no signed overflow, and **deleted `while (x < 10) {}`**, returning past it. Both
promises are broken under `cl.exe` without help. It has no flag for either: the
undocumented `/d2UndefIntOverflow-` is refused by 14.38.

- **Wrapping, for every compiler** (the user's choice): signed `+`, `-` and `*`, the same on
  `u8` and `u16` (which C++ promotes to `int`, so `u16 * u16` overflowed too), unary `-`
  of a non-literal, and `+=`, `-=`, `*=` are computed in the unsigned type and cast back.
  It is one instruction on every compiler (`cl /O2` gives `lea eax, [rcx+rcx]` for the
  doubled value). A compound assignment to a plain name is written out; to any other place,
  through `wrapping_*_assign`, so the place is evaluated once. `-fwrapv` stays on GCC and
  Clang **until the benchmarks show it makes no difference**, then goes.
- **Loops, a barrier only where there is no effect** (the user's choice): a `while` or `loop`
  whose condition and body call nothing and store nothing but into the function's own
  locals starts with `DZ_LOOP_ALIVE()`, which is `_ReadWriteBarrier()` under `cl.exe` and
  nothing elsewhere. That kept the probe's loop. Not every loop, since a barrier in each
  would keep `cl.exe` from vectorising ordinary ones. `-fno-finite-loops` stays on GCC and
  Clang.
- **Aliasing**: MSVC does no type-based alias analysis, so `-fno-strict-aliasing` has no
  counterpart and needs none; T-0253 says so where it writes `cl.exe`'s flags.

Still to do: the benchmark run before `-fwrapv` is dropped.

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
