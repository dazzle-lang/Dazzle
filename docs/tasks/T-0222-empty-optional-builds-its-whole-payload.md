---
id: T-0222
title: "raytrace's gap: the shape of the generated code (struct literals, expression temporaries, optionals)"
milestone: M15
status: open
kind: feature
area: codegen
where: `crates/dz_cpp/src/lib.rs`: `switch` (the union `else`), struct literals, `if`/`switch` expressions (`_dz_val`), `if let` and `or` bindings; `crates/dz_cpp/src/runtime.rs`: `Opt`; `crates/dz_check/src/check.rs`: `literal_on_left`
---

What T-0221 left of `raytrace`'s gap (T-0174). With private functions `static`, it runs
1.10× its C++ half on both compilers (`RESULTS.md`, 2026-11-14): 500 ms against 454 on
clang++, 687 against 623 on g++. Each experiment in the table is a whole run, after editing
a copy of the generated C++ or the C++ half and building it with the same flags; none of
them explained the rest. A profile followed, once `perf` was allowed (as root, `sysctl
kernel.perf_event_paranoid=1`), and is below the table.

Ruled out, 2026-11-14 (clang++ / g++, ms, against 500 / 689 for the Dazzle build):

| Tried | clang++ | g++ |
|---|---:|---:|
| An empty `Opt<T>` of a trivially copyable `T` holding its payload in a union, so `none` builds nothing, as `std::optional` does (this task's first idea; the C++ half with Dazzle's shape of optional had been 27 ms slower on g++) | 526 | 687 |
| `std`'s `Vec3` methods and operators all `#[inline]`, so they inline before LTO; the C++ half with its `Vec3` in a file of its own was 476 / 624 against 454 / 622 | 490 | 704 |
| `float_to_int`'s saturating checks replaced by a plain cast | 488 | 701 |
| The `Hit` taken out of an optional bound by reference instead of copied | 485 | 707 |
| (the C++ half without its anonymous namespace, so its methods have external linkage as Dazzle's do) | 461 vs 454 | 623 vs 622 |

The last three were measured on top of the `#[inline]` `Vec3`. None was kept: the union
was slower on clang++, and `#[inline]` on `Vec3` traded 10 ms on clang++ for 15 on g++.
`SliceIter`'s methods, which the shape loop calls, are already inline in the header.

**Profiled, 2026-11-14** (clang++, `perf`, precise sampling with AMD IBS, `cycles:pp`):

- `perf stat`: the Dazzle half retires 6.38 billion instructions against 5.77 (+10%) at the
  same 2.59 per cycle, with 1.81 billion L1 loads against 1.47 (+23%). It is doing more
  work, not waiting on misses: branch misses are 0.60% against 0.58%.
- By function, in cycles: `Scene::closest` +2900 samples of 50 000 cycles (about 30 ms),
  `powf` +5100 (about 50 ms), `trace` and `expf` together −3300. **`powf` does exactly the
  same work in both**: an `LD_PRELOAD` shim counting its calls and hashing every argument
  in order gave the same count, exponents and hash. Its extra cycles are spent waiting on
  the caller, so they are `trace`'s, not libm's; clearing `xmm2` before each call (its
  first instruction, `vcvtsi2sd`, merges into it) changed nothing.
- **`closest`'s miss path stores 32 bytes of zeros per shape**: `return none` from the
  inlined `intersect` builds an empty `Opt<Hit>`'s payload, two 16-byte stores the C++
  half's loop does not have. The union `Opt` above removes them: 220 million fewer
  instructions, 165 million fewer loads, `closest` about 23 ms faster. But `trace` and
  `powf` then took about 33 ms and 25 ms more, with `trace`'s code the same size and
  making the same calls, so the cost moved into how `closest`'s result (an `Opt<Hit>` of
  48 bytes, returned through memory) is written and then read; most likely a store that
  cannot be forwarded to the load after it.
- Building `intersect`'s two `Hit` literals by aggregate initialisation instead of the
  zero-then-assign lambda: no change, with either `Opt`.

**Followed up the same day**, with AMD's load/store counters (`ls_dispatch.store_dispatch`,
`ls_stlf`, `ls_bad_status2.stli_other` for a load that cannot be forwarded from a store,
`ex_no_retire.load_not_complete`) and IBS op sampling (`ibs_op/cnt_ctl=1/`, which counts
retired operations precisely: libm's counts agree between the halves to 1%):

| | stores | forwarded | not forwardable | retire waiting on a load | clang++ |
|---|---:|---:|---:|---:|---:|
| Dazzle | 713 M | 548 M | 77 M | 168 M | 500 ms |
| Dazzle, union `Opt` with a `char` placeholder | 555 M | 475 M | 104 M | 459 M | 526 ms |
| Dazzle, union `Opt` with nothing initialised | 525 M | — | 71 M | 175 M | 499 ms |
| C++ | 396 M | 304 M | 42 M | 39 M | 455 ms |

- **Why the first union `Opt` was slower**: its default constructor wrote the one-byte
  placeholder (`movb $0, (%rdi)`) and clang then read the payload's first `float` from the
  same address, a 4-byte load after a 1-byte store, which cannot be forwarded: 73% of the
  load stalls in `closest` were that one instruction. With no placeholder (an anonymous
  `union { T value; }` and `OptStore() {}`) the stall is gone, 170 M fewer instructions
  and 190 M fewer stores than today's build, warning-clean on both compilers, and **no
  faster** (499 ms; g++ 686 against 689). The CPU retires about 2.5 instructions a cycle
  and had room for the waste, so it was never the limit. Not landed: it removes work
  without saving time, and changes every program's optionals.
- **Not the union**: the C++ half with `Material` as a Dazzle-style union copied as bytes
  is as fast (452 ms); with `Shape` too, 468 ms on clang++ but 585 on g++, 42 ms *faster*;
  with Dazzle's `Opt` on top, 469 and 626. So the C++ half shaped like Dazzle's code is
  still 30 ms (clang++) and 63 ms (g++) ahead.
- **Not inlining**: the C++ half's `trace` calls `closest` out of line once and inlines the
  shadow ray's call; Dazzle's calls it twice. But forcing Dazzle's `closest` inline is
  much slower (584 ms), an `inline` hint changes nothing, and the C++ half with `closest`
  `noinline` is 467 ms on clang++ and 591 on g++, faster than with it inlined.
- Padding `Material` to 32 bytes, so its copy is two halves that do not overlap: no faster,
  and more failed forwards (83 M). `best = _dz_opt` without the `hit` copy: 110 M fewer
  stores, no faster.

**Where it stood then**: every structural difference found was worth 15 ms or less, one way
or the other depending on the compiler, and the extra work fitted in the CPU's spare width.

**The bounding experiment, the same day.** The generated `intersect`, `Scene::closest`,
`Scene::sky` and `Scene::trace` were rewritten by hand as clean C++ on exactly Dazzle's
types (`std`'s `Vec3` and its functions, `Opt`, the generated unions, `List`), with the
same float operations in the same order: aggregate literals, plain conditionals,
references, an index loop, no lambdas or temporaries. Same checksum, and:

| | clang++ | g++ |
|---|---:|---:|
| Generated | 501 ms | 688 ms |
| The four functions written cleanly | 475 | 625 |
| C++ half | 455 | 624 |

So **the shape of the generated code was most of the gap**: all of it on g++, 26 of 46 ms
on clang++, the rest presumably the layout of the types (a union `Shape` alone cost the C++
half 16 ms on clang++). Clean `trace` gave clang++ its part and nothing on g++; clean
`intersect` and `closest` gave g++ its part and nothing on clang++.

Taken one at a time from the generated code, most shapes did nothing alone, or helped one
compiler and hurt the other: an index loop for `closest`, the `Hit` bound as a `const&`,
`if` expressions as `?:`, aggregate `Hit` literals, and `best` kept as the optional (39 ms
*slower* on g++ alone, though the clean version does the same). One was a clear gain on
both, and is built: **a union's `switch` with no `default` ends in `else`** (498 → 486 ms on
clang++, 688 → 668 on g++), since testing the last variant again gave the C++ compiler a
branch that cannot be taken and a path past the chain ending in `unreachable`.

The experiment also found a **checker bug**, fixed with it: a number written on the left
of an operator, `0.5 * (dir.y + 1)` in `sky`, stayed untyped while the operator became
`f32`, settled on `f64`, and was emitted as `0.5`, so the C++ computed in `double` and
narrowed. Spec §2.5 already says the literal takes its type from the expression around
it; it now does from the right operand as from the left. `std`'s `Random::unit_vec3` took
its square root in `double` because of it. `tests/run/literal_left` pins it with values
that differ between the two (`0.1 * x` for x = 9 is 0.899999976 one way and 0.900000036
the other).

With both, `raytrace` is 1.07× its C++ half on both compilers (`RESULTS.md`). **What is
left** is the remaining shape of the generated code, whose parts act together rather than
one at a time, so the next step is a code-generator change that removes several at once,
measured as a whole: struct literals as aggregate initialisation rather than a lambda that
zeroes then assigns, `if` and `switch` expressions without `_dz_val` temporaries, and a
`let` taken out of an optional bound in place. Clean code reaches 475 / 625 ms, so about 10
ms on clang++ and 40 on g++ is there to win; the empty `Opt` with nothing initialised (no
time here, 170 M fewer instructions) belongs in the same change.
