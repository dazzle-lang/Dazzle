# M21: MSVC

`cl.exe` was never refused for a language reason: `dz` spoke only GCC's command line, and
the runtime used GCC's and Clang's builtins. The user put it in 0.3 on 2026-11-18 (spec §19):
T-0254 the runtime, T-0255 the three flags that carry meaning, T-0253 the toolchain, T-0256 a
CI row warning-clean at `/W4`.

## 2026-11-19: the runtime without builtins, the three flags, and `dz` driving `cl.exe`

**The runtime (T-0254).** Every builtin `cl.exe` lacks now has one spelling in
`dz_runtime.h`, chosen by `DZ_MSVC` (`_MSC_VER` without `__clang__`, since clang-cl has the
builtins): `__builtin_*_overflow` became `add_overflows`, `sub_overflows` and
`mul_overflows`, portable checks under `cl.exe` (a 64-bit product is undone by division:
only `checked_mul` reaches it); `popcount` is counted in parallel rather than with
`__popcnt64`, which `cl.exe` emits without checking the CPU has it; `ctz` and `clz` are
`_BitScanForward64` and `_BitScanReverse64`; NaN is `f != f`, since `std::isnan` is not
`constexpr` in C++17; `Random`'s 128-bit product is `__umulh`; `__builtin_unreachable` is
`DZ_UNREACHABLE()`, `__assume(0)`; and `blackbox` escapes its value's address to a `volatile`
pointer behind `_ReadWriteBarrier()`, since a volatile copy did not compile for a struct.
The same pass found `wrapping_mul` on a `u16` was undefined everywhere: both operands are
promoted to `int`, where 65535 × 65535 overflows. It and the others now compute in at least
`unsigned`'s width.

**The flags that carry meaning (T-0255), measured first.** `cl /O2` folded `x * 2 / 2` to
`x` and deleted `while (x < 10) {}`, so both of Dazzle's promises broke under it, and it has
no flag for either (14.38 refuses the undocumented `/d2UndefIntOverflow-`). The user chose:

- signed `+`, `-`, `*` (and on `u8`, `u16`), unary `-` and the compound assignments are
  emitted through the unsigned type **for every compiler**, one instruction on each.
  `-fwrapv` stays on GCC and Clang until the benchmarks show it changes nothing;
- a `while` or `loop` with **no effect**, which calls nothing and stores only into the
  function's locals, starts with `DZ_LOOP_ALIVE()`: `_ReadWriteBarrier()` under `cl.exe`,
  nothing elsewhere. Not every loop, since a barrier would keep `cl.exe` from vectorising.

MSVC does no type-based alias analysis, so `-fno-strict-aliasing` needs no counterpart.

**The toolchain (T-0253).** `crates/dz/src/toolchain.rs` holds everything that differs
between the two command lines, and the build asks it for each flag. `cl` is told by its name;
from an ordinary shell `find-msvc-tools` supplies the native compiler and Windows SDK.
The result is reused within the process. Objects are named by
`/Fo`, the program by `/Fe`, and `cl.exe`'s `/sourceDependencies` JSON stands in for the
make-style depfile, so the object cache and the build stamp work unchanged: a rebuild with
nothing changed is up to date in about 10 ms. LTO is `/GL` and `/LTCG`; debug information is
`/Z7`, in each object, since parallel compiles cannot share one `.pdb`. `cl.exe` writes its
diagnostics to standard output, after a line naming the file, which is dropped. A recipe's
flags are written in GCC's spelling and translated (`-std=c17`, `-include`, `-D`, `-I`, `-w`).
Precompiled headers are not used under `cl.exe` yet: `/Yc` and `/Yu` work differently from
`-include`, and a precompiled header is only ever an optimisation (T-0181).

Two findings on the way, both from building every `tests/run` program with `cl.exe`:

- **NRVO.** Five programs printed one `drop` more than expected, of a value at its defaults.
  A struct literal is built in a lambda that returns a named local, and only NRVO, which C++
  allows but does not require, keeps that local from being moved from and dropped. clang++
  and g++ always apply it; `cl.exe` only at `/O2`, or with `/Zc:nrvo`, which `dz` now
  passes. The generated code still rests on it: T-0260.
- **MSVC 14.50 crashes** (C1001) on a field default `Array<Vec4, 4> cols =
  Array<Vec4, 4>{{Vec4{}, …}}` once the struct is built as a temporary (`Mat4{}.inverse()`);
  14.38 does not. Such a default is now written `cols{{…}}`, which means the same.
- **`0.0 / 0.0`**, a NaN, is refused by `cl.exe` as a constant division by zero (C2124). A
  float divided by a literal zero now goes through `num::fdiv`, which it does not fold.

What was checked: every `tests/run` program, its C++ compiled by hand with MSVC 14.38 at
`/Od` and at `/O2` (those without a C library, which the script did not build); and the
`tests/run` harness with `DZ_CXX=cl`, which found Visual Studio's newest, 14.50, through
`vswhere`: every program passes but `cmake`, which needs Ninja, not installed on that
machine. `dz run --cxx cl` builds and runs debug, release and shipping. Every example builds
but `box2d`, whose own CMake compiles one file with `/arch:AVX2` under MSVC, which a recipe
cannot yet say. Warnings at `/W4` (C4127 constant conditions, C4702 unreachable code, among
others) are T-0256's.

## 2026-11-19 follow-up

The Windows integration run passed on the x64 MSVC 14.50 toolset at `/W4 /WX`, including
the feature matrix, incremental rebuilds, benchmark checksums and every example. Box2D
now uses its upstream CMake project, which applies AVX2 to its wide solver. Its relocatable
pkg-config file also exercises `${pcfiledir}` in the installed header and library paths.
The same example built with Windows clang++. The portable arithmetic regression passed in
debug and release with MSVC, clang++ and g++.

With compiler and SDK paths cleared, automatic discovery built and ran hello-world in
debug, release and shipping modes. A CMake project in a directory containing spaces also
ran with Visual Studio's bundled CMake and Ninja. The Windows CI job runs the program
suite, feature matrix and incremental checks with MSVC. Its first hosted run is pending.
