---
id: D-0107
title: "#[inline] inlines every call in an optimised build"
status: decided
---

**`#[inline]` on a function or method with a body makes every call to it inlined in an
optimised build, as Rust's `#[inline(always)]` does. Chosen by the user on 2026-11-17, in
T-0240 ("#[inline] should behave like #[inline(always)]"); spec §12.8, r137.**

**Before.** `#[inline]` put a body where every calling module sees it (r106, T-0153), and
the C++ compiler decided whether each call was worth inlining.

**Why.** `ecs`'s query is a few small methods, `seek`, `get`, `valid` and `advance`, called
once per element of loops nested two deep. With bounds checks, clang++ priced `seek` at 410
against its threshold of 325 and kept it a call; the C++ half's, without checks, it also
kept, but its `get` it inlined, where Dazzle's it did not. Writing the methods `#[inline]`
changed nothing while the C++ compiler still decided (212 ms on clang++). Forced, the same
program ran in 129 ms, faster than its C++ half (188). A library author who marks a method
`#[inline]` has already said it is small and hot; the C++ compiler's cost model, which
counts each bounds check's failure call as a full call, is not a better judge of that.

**What follows.**

- Codegen writes such a body `DZ_INLINE` (`inline __attribute__((always_inline))`,
  `__forceinline` on MSVC), the runtime's own spelling for what must vanish, in an
  optimised build. A debug build still inlines nothing, and keeps an instance's body out of
  the header (T-0214).
- **Recursion.** C++ cannot inline a function into itself, and g++ makes forcing one an
  error. A body that can reach itself through calls to `#[inline]` bodies keeps plain
  `inline`, the C++ compiler's choice. Calls are followed conservatively (a method of an
  implementation by its name), so a cycle may be seen where there is none; never missed.
- **The cost is the programmer's.** A large `#[inline]` function is copied into every
  caller. Measured on the benchmarks, with `std`'s own `#[inline]` functions forced, g++
  ran `json` 5.9% fewer instructions and `words` 3.4% more, where inlining `String`'s
  methods made `generate` small enough for g++ to inline into `main`; clang++, which
  already inlined them, did not move.

**Not decided.** An `#[inline(never)]`, or a way back to the C++ compiler's choice, if a
program needs one.
