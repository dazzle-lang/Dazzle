---
id: T-0254
title: The runtime without GCC and Clang builtins
milestone: M21
status: done
kind: feature
area: codegen
where: `crates/dz_cpp/src/runtime.rs` (`checked_add/sub/mul` ~727, the float-to-int cast ~762, saturating ops ~771, `count_ones`/`trailing_zeros`/`leading_zeros` ~797–807, `Random`'s range ~828, `blackbox` ~1098); `crates/dz_cpp/src/lib.rs` (`__builtin_unreachable` ~7677)
---

Part of M21. The runtime and the emitter use builtins `cl.exe` does not have:

| Used | Under `_MSC_VER` without `__clang__` |
|---|---|
| `__builtin_add/sub/mul_overflow` | a portable check, or `_addcarry_u64` / `_mul128` |
| `__builtin_popcountll`, `ctzll`, `clzll` | `__popcnt64`, `_BitScanForward64`, `_BitScanReverse64` |
| `__builtin_isnan` | `std::isnan`, or `f != f` |
| `unsigned __int128` | `_umul128` / `__umulh` |
| `__builtin_unreachable()` | `__assume(0)` |
| `asm volatile` in `blackbox` | its existing volatile fallback, or `_ReadWriteBarrier` |

`DZ_INLINE` already spells `__forceinline` for MSVC. Each replacement is behind one macro or
function in the runtime, so the emitter keeps writing one spelling, and the existing tests
of each operation run on every compiler.
