---
id: T-0141
title: `virtual` and `override` methods take no result placement
milestone: none
status: open
kind: feature
area: ownership
---

Split out of [T-0117](T-0117-memory-placed-by-compiler-regions-result.md), stage 2,
round 2 ("Still out: `virtual` and `override` methods").

**Where the code is**

| Piece | Where |
|---|---|
| Which functions take a placement | `result_functions` in `dz_flow/src/placement.rs` |
| How trait methods got one, which this follows | `dyn_signature`, `passes_result` in `dz_cpp/src/lib.rs` (stage 2, round 2) |
| Virtual dispatch | the `virtual` and `override` emission in `dz_cpp/src/lib.rs` |

**What it is.** A `virtual` method returning something that owns memory, such as a `List`
or a `String`, always returns on the heap. That holds even where every override could build
the result where the caller keeps it.

**Why it matters.** It matters for speed only. Class hierarchies with `virtual` methods are
how struct inheritance (D-0032) is used for game objects and UI, so this reaches real code.

**Why it is this way.** A base method and its overrides share one C++ signature, so they
must take a placement together or not at all. Round 2 did this for trait methods, where the
trait's signature decides; `virtual` was left until a base and its overrides could be
decided as one.

**What it would take.** A base's signature decides, as a trait's does: when the base's
result keeps an allocator, the base and every override take `_dz_result`. Compiler work
only (`docs/log/`).
