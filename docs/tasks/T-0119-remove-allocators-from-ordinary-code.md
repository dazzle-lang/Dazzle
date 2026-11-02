---
id: T-0119
title: Remove allocators from ordinary code
milestone: none
status: done
done: r104, 2026-11-02
kind: feature
area: stdlib
---

Step 6 of D-0091's plan (spec §14.6), split out of
[T-0107](T-0107-design-ties-kills-clauses-compiler-placed.md); after
[T-0117](T-0117-memory-placed-by-compiler-regions-result.md).

**Where the code is**

| Piece | Where |
|---|---|
| The constructors without an allocator; `List`, `HashMap` and `Box` store `#placement`, `Rc` takes the heap | `std/collections/list.dz`, `hash_map.dz`, `hash_set.dz`, `slot_map.dz`; `std/text/string.dz`; `std/io/path.dz`; `std/memory/box.dz`, `rc.dz` |
| `#placement`: the name, its type, `DZ0528` outside `unsafe`, `DZ0579` as a default (`in_param_default`) | `INTRINSICS` in `dz_ast/src/lib.rs`; `placement` in `dz_check/src/check.rs`; `PLACEMENT_DEFAULT` in `dz_diag/src/codes.rs`, explained in `dz_diag/explain/check.md` |
| `#placement` in C++: `_dz_result`, or the heap | `intrinsic` in `dz_cpp/src/lib.rs` |
| `format!` takes nothing by name | `format_named_args` in `dz_check/src/check.rs` |
| Which calls are sites: a call to a function that takes a placement, `Rc(…)`, a call leaving an allocator parameter at its default | `callable`, `result_functions` in `dz_flow/src/placement.rs` |
| The hand-placed build beside heap and regions | `--hand-src`, `--hand-dz` in `tools/bench_regions.py` |

**What it is.** With the compiler placing memory, std's allocator parameters, the examples'
and the benchmarks' `Allocators::Temp` and `#allocator` go. The `Allocator` trait,
`ScopeArena` and `TempArena` stay as `std` internals under `unsafe`.

```dz
let words = List<String>(Allocators::Temp)   // today
let words = List<String>()                   // after: placed by the compiler
```

**Why it matters.** It is D-0091's other half: no allocator to choose means none chosen
wrong, and ordinary code stops carrying allocator parameters.

**Why it is this way.** It cannot come before placement, or code would lose the speed hand
placement gives it now.

**What it would take.** std's container constructors and every caller, the examples and the
benchmarks; the prototype's `STORES`/`TAKES` tables and `#allocator` went with it in r90
(T-0117). Measured against hand-placed `Temp` as in T-0104: stage 3 reached `words`' keys but
gained 2%, with hand-placed `Temp` 6–7% ahead, which this must not lose.

**Settled by the user, 2026-11-02, before it was built:**

- **How a container learns where it was placed:** the intrinsic `#placement`, a `^mut
  Allocator`, in any `unsafe` code, not `std`'s alone, so a program's own container is
  placed as `std`'s are (recorded in D-0091). The other way offered, a hidden parameter only
  `std` could pass, kept two mechanisms for one placement.
- **The speed gate:** do it now and record the loss, rather than first making placement
  reach `words`' keys and `expr`'s nodes. Placement being automatic, checked and right comes
  first, as on 2026-11-01. The two gaps are
  [T-0145](T-0145-value-stored-into-container-built-where.md) and
  [T-0146](T-0146-value-rebuilt-on-each-pass-loop.md).

**Done, r104 (2026-11-02).** `docs/log/r81-ties.md` has what was built. No constructor in
`std` takes an allocator, `format!` takes nothing by name, and the examples, the benchmarks
and `tests/` name no allocator but where they test the allocators themselves.

**Measured against hand-placed `Temp`, as T-0104 was.** `python tools/bench_regions.py
--hand-src … --hand-dz …`, `--release`, the median of ten runs of each build, rotating
which goes first. *Hand* is each program at `39680ee`, its scratch in `Allocators::Temp`,
built by that commit's `dz`; *heap* and *regions* are the programs as r104 leaves them,
with `DZ_REGIONS` off and on. AMD Ryzen 7 H 255 (16 threads), Linux, glibc 2.44; clang
23.1.4 and g++ 16.2.1. The machine was busy when the run began (load 10.6, 3.9 at its end),
so the fastest run of each build is given too; the gaps hold there.

| Program | Compiler | Hand | Heap | Regions | Regions / hand | Fastest: hand / regions | Peak: hand / regions |
|---|---|---:|---:|---:|---:|---:|---:|
| astar | clang++ | 705 ms | 703 ms | 700 ms | 0.994 | 696 / 686 ms | 19.9 / 20.1 MiB |
| astar | g++ | 578 ms | 568 ms | 559 ms | 0.968 | 558 / 544 ms | 19.8 / 20.1 MiB |
| words | clang++ | 399 ms | 446 ms | 442 ms | **1.107** | 386 / 420 ms | 70.9 / 46.5 MiB |
| words | g++ | 365 ms | 427 ms | 418 ms | **1.145** | 352 / 382 ms | 70.4 / 46.4 MiB |
| expr | clang++ | 357 ms | 369 ms | 376 ms | **1.053** | 346 / 370 ms | 141.1 / 141.2 MiB |
| expr | g++ | 283 ms | 301 ms | 304 ms | **1.074** | 278 / 297 ms | 141.3 / 141.3 MiB |

The loop rule's row holds: a million passes peak where ten thousand do (4.0 against 4.1
MiB at most), on both compilers.

**Read against the gate:** `astar` loses nothing, since it never used `Temp`. `words` is
9–15% slower than hand placement and `expr` 5–7%, where the task asked to lose nothing: the
cost the user accepted to do this first. Regions are even with the heap on both, because
placement does not reach their hot allocations: `words`' keys are copies of a buffer
stored into the map ([T-0145](T-0145-value-stored-into-container-built-where.md)), and
`expr`'s nodes are rebuilt on each pass of the parser's loops and taken apart by a tuple
`let` ([T-0146](T-0146-value-rebuilt-on-each-pass-loop.md)). What hand placement bought
`words` it paid for in memory: `Temp` kept every buffer the text and the map outgrew until
the end, 70 MiB at peak against 46.
