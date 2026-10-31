---
id: T-0117
title: Memory placed by the compiler: regions and the result placement
milestone: none
status: open
kind: feature
area: ownership
---

Step 4 of D-0091's plan (spec §14.6), split out of
[T-0107](T-0107-design-ties-kills-clauses-compiler-placed.md), which has the full design and
the scenarios it was checked against.

**Where the code is** (keep this current; stage 1's round 1, below, built it).

| Piece | Where |
|---|---|
| The region arena: `ScopeArena` (chunks doubling; a block of 16 KiB or more is its own, freed at once) and `ScopePool` (per thread; large blocks by size class up to 4 MiB) | `dz_project/src/std/memory/scope_arena.dz`, `scope_pool.dz`, listed in `STD_SOURCES` (`dz_project/src/lib.rs`) |
| A copy of a region value goes to the heap: `Allocator::scoped` (false; `ScopeArena` says true), read by `List::copy` and `HashMap::copy`; `String`, `Path`, `HashSet` and `SlotMap` copy through them | `std/memory/allocator.dz`, `std/collections/list.dz`, `std/collections/hash_map.dz` |
| Sites, the proposer and the reasons' words: `place`, `place_with` (`Proposer::Rules`, `Proposer::Everything`), `Placements`, `BodyPlacement` (`region_blocks`, `region_for`, `demoted`), `Site`, `Decision` | `dz_flow/src/placement.rs` |
| The ties check: `analyse_regions`; `Value::owned`, `Regions`, `Refusal`; `region_site` (the loop rule), `region_assign`, `region_end`, `sink`, `passed` (a copy carries no region) | `dz_flow/src/ties/body.rs` |
| One answer for every reader: `Analysis::placements`, computed after ownership and timed as `placement` | `dz_driver/src/lib.rs` |
| Tests | `dz_flow/tests/placement.rs`; `tests/run/region_arena` (the arena by hand and the copy guard, also under `--track-memory` and `-Wall -Wextra`) |
| The gate's stopwatch | `tools/bench_regions.py` |
| Not built yet: codegen, the inlay hint, `dz placements` | round 2, below |

**What it is.** No allocator appears in ordinary code: the compiler puts each value in the
most efficient place it can prove safe, the heap last.

| The value | Its memory |
|---|---|
| never leaves its scope | that scope's region |
| returned | wherever the caller wants the result (a hidden result placement) |
| anything else | the heap |

A value's owned parts live where the value lives; a constructor and `copy` build into
wherever `self` is placed. Every function returning something that owns memory takes a
hidden result placement — free functions, methods, trait methods and function values alike.
Where a by-value parameter ends up is inferred within a module; across modules, and through
anything the compiler cannot see, the default is the heap. With the ties checker, a wrong
placement fails the check and falls back to the heap.

**Why it matters.** It removes the class of bug where a value outlives its arena, and the
choice that is easy to get wrong (T-0099: `Temp` was slower than the heap for a growing
container). In the prototype, scope regions and result placement made `astar` 12–17% faster
than the heap.

**Why it is this way.** The regions prototype ([T-0104](T-0104-prototype-compiler-chooses-where-memory-comes.md),
[T-0105](T-0105-placement-follows-value-destination-not-function.md)) placed memory with no
checker, and each stage's safety rested on rules a test had found; ties are that checker.

**What it would take.** Stages 1 and 2 of the prototype re-based on ties, the result
convention for every function, constructors and `copy` building into `self`'s placement, and
what `dz` shows for each placement. Should follow D-0093's steps, since placement reasons
about the same views. Then [T-0118](T-0118-skip-drops-that-only-free-region.md) and
[T-0119](T-0119-remove-allocators-from-ordinary-code.md).

**Agreed 2026-10-31: scope regions first, behind a go/no-go gate.** Ties stay whatever
happens here; regions are kept only if they earn it. Build scope regions alone (no result
placement yet), then measure:

| Gate | Passes when |
|---|---|
| a loop reassigning a `String` 1M times (`s = format!(…)`) | peak memory stays flat |
| `tests/bench_programs` `astar`, `words`, `expr` | each faster than the heap |
| peak memory on the same | within 10% of the heap's |
| every wrong placement | the ties check refuses it, and the value goes to the heap |

If any gate fails, regions are dropped: allocators stay as §14 to §14.2 write them, and
T-0118 and T-0119 close as not doing.

**Stage 1 plan (2026-10-31), in order:**

1. Bring back the region arena from the tag `regions-prototype` (`548b771`): `ScopeArena`
   (chunks doubling), `ScopePool` (per thread), in `std/memory/`, under `unsafe`, for the
   compiler only.
2. At `let x = List()`, `String(…)` and any call taking an allocator parameter it leaves at
   its default, give the call the scope's region when `x` is only borrowed afterwards (the
   prototype's rule, `dz_cpp/src/regions.rs` at the tag); declare the region first in the
   block, so it is released after everything in it drops. Reassignments keep the heap, which
   is the loop rule.
3. A copy of a region value goes to the heap (the prototype's stage-1 hole, solved there by
   `Allocator::scoped`): `List::copy` and `String::copy` must not inherit a region.
4. Show each placement: an inlay hint, and `dz placements <dir>` as text or JSON, from the
   one pass that decides.
5. Run the gate above and record the numbers here; keep regions or drop them.

**The loop rule, from T-0104, missing until now:** an allocation that can run more than once
while its region is alive goes to the heap, unless the value it makes dies within the same
pass. Without it, `s = format!(…)` in a loop, or `self.items = List()` every frame, grows the
outer region without bound, which is where region inference failed before (the ML Kit). The
first gate row tests it.

**Stage 1, round 1, built 2026-10-31: steps 1 and 3, and the analysis half of step 2.**
Uncommitted on the branch `t-0104-regions-prototype`. No program runs differently yet:
nothing reads the placements but the tests.

- **Step 1, the arena.** The tag's `ScopeArena` and `ScopePool`, brought up to today's std
  (r81–r102): `realloc` returns `^mut u8 in (self, p)`, and an untied pointer becomes a
  reference only in `unsafe`. One fix: an old large block never grows in place. `ScopeArena`
  is `pub`, as `TempArena` is, and its comment says it is the compiler's; `ScopePool` is
  private to std. Like `TempArena`'s, its memory is not tracked by `--track-memory`.
- **Step 3, copies.** `Allocator::scoped()`, and `List::copy` and `HashMap::copy` take the
  heap when the source's allocator is scoped. No other function in std builds a value with
  another value's allocator. With the guard switched off, `tests/run/region_arena` reads
  `0xEE` from its copies.
- **Step 2, the analysis.** `dz_flow::placement` decides every site of every body, in
  analysis, so the editor, `dz placements` and codegen read one answer:
  - A **site** is a call to std with an allocator parameter left at `Allocators::Global`.
    A call to the program's own function with one is a site too, always heap (stage 2).
  - **The proposer** is the prototype's stage-1 rule: `let x = site` in block `B` gets
    `B`'s region when every later use leaves `x` where it is. A copy leaves it
    (`counts.insert(word, 1)` copies `word`); `move(x)`, `return x` and a block's value do
    not. Only a `let`'s value is proposed, so the loop rule holds by construction.
  - **The ties check** walks the body's ties again, a region being a tie no type declares
    that dies at the end of its block, after the block's own locals. It refuses a value
    that is returned, stored outside the block, passed by value, still held when the block
    ends, or made on every pass of a loop the region outlives. It shares no rule with the
    proposer, and its findings are never diagnostics.
  - Heap by a fixed rule: `Rc` (a `Weak` can outlive the scope), a formatting macro's body
    (C++ gets it as a lambda), initialisers, parameter defaults and base-constructor
    arguments.
- **Counts**, from the corpus test: 63 of the 72 programs check in a fresh worktree (the
  rest need the vendor submodules), and the check refuses nothing the proposer placed.
  `astar` places 3 of 3 sites (both maps and the path), `expr` 1 of 4, std 2 of 22. `words`
  has none, since it names `Allocators::Temp`; its heap version, which the gate measures, is
  not counted yet.
- **Checked**, both halves together: the `dz_flow`, `dz_check`, `dz_driver` and
  `dz_project` tests, clippy with `-D warnings`, `cargo fmt`, and `dz check` on all nine
  examples. The arena alone: `cargo test -p dz --test run`, 44 of 45, the one left being
  `every_example_builds`, for want of submodules; `region_arena` on clang++ and g++. The
  analysis alone: the `dz_lsp` tests. The new `placement` phase broke
  `timings_name_every_phase_on_standard_error`, which now lists it, as
  `tools/bench_programs.py` does.
- **Edges found, for round 2:**
  - A copy is safe only while every copy goes through `copy`: the C++ copy constructor and
    copy assignment alike, for `String`, `Path`, `HashSet` and `SlotMap` too. And codegen
    must not turn a last use into a C++ move.
  - The proposer sends some safe cases to the heap that the check would accept:
    `let y = move(x)` in the same block, `for e in move(xs)`, temporaries such as
    `f(&List())`, a struct literal bound in the same block, and `let x = … or …`.
  - The check does not follow an element taken out by index (sound while an element owns
    its own memory), a `switch` subject, or a `case` binding taken by value. A generic body
    is decided once, with `T` taken as not copyable.

**What is left of stage 1, in order.**

1. **Round 2, two items that can run side by side.**
   - **Codegen, behind a switch.** `dz_cpp::Options::regions`, on with `DZ_REGIONS=1`, read
     by `dz build` and `dz run` as the prototype's `crates/dz/src/build.rs` did, so
     `tools/bench_regions.py` builds one source both ways. `emit` is handed
     `Analysis::placements`. In `block_into`, a block in `region_blocks()` first declares
     `ScopeArena _dz_regionN{};`, qualified from the prelude, so C++ releases it after the
     block's locals; in `call`, `region_for(e)` puts the region where the allocator argument
     was. An instance body from `dz_mono` keeps the generic body's `ExprId`s, so decisions
     are looked up by the body's `BodyOwner`. The prototype's version of both is commit
     `1c94649`. Tests: `regions_change_nothing` (every `tests/run` program prints its
     `expected.txt` with regions on, on clang++ and g++, warning-clean), and the prototype's
     `tests/run/regions` cases, taken from the tag.
   - **Showing each placement.** An inlay hint at each site, `region: …` or `heap: …`: a
     new `HintKind` beside `clause_hints` in `dz_lsp/src/ide.rs`, sent by `inlay_hints` in
     `dz_lsp/src/lib.rs`. And `dz placements <dir>`, as text or with `--format=json`, in
     `crates/dz/src/main.rs`. Both print `Site::decision` as it is. Tests in
     `dz_lsp/tests/ide.rs` and for the command.
2. **Round 3, the gate.** `cargo build --release -p dz`, then `python tools/bench_regions.py`
   (clang++ and g++, ten runs each; `--markdown` and `--json` keep the report) for the
   first three rows; `the_region_check_alone_refuses_every_wrong_placement` is the last.
   Record the numbers here. Then the user keeps regions or drops them.

`tools/bench_regions.py` builds the heap version of each benchmark (every
`Allocators::Temp` taken out, as T-0104 did) twice, without and with `DZ_REGIONS=1`, checks
that both print the same checksum, and reports the medians and peak memory; and a loop that
reassigns a `String`, a `List` and a `format!` result, whose peak at a million passes must
be within 1 MiB of its peak at ten thousand. It runs today, but measures nothing until
codegen reads the switch.

**Open before the gate: the user's call.** T-0104 measured stage 1 alone: `words` took
400 ms against 398 on the heap, and `expr` 370 against 371. Stage 1 does not reach their
hot allocations (`words`' keys and text, `expr`'s `Box` nodes, made through its own
`Parser`). Read as written, "each faster than the heap" would drop regions over those two
rows even if `astar` gains 15%. Do those rows mean "faster", or "not slower"?

**Settled in r90:**

- **The prototype is removed, all three stages**, and kept at the git tag
  `regions-prototype` (commit `548b771`) to mine: `dz_cpp/src/regions.rs`, the `#allocator`
  intrinsic, `contents_allocator`, `String::copy_in`, `Allocator::scoped` and `contents`,
  `ScopeArena`, `ScopePool`, `ScopeShared`, `DZ_REGIONS` and `tests/run/regions`.
  `#allocator` was live in the checker and in std's constructors although D-0091 rejects it,
  and every rule the stages placed by is replaced by ties here anyway. The A/B numbers stay
  in T-0104 and T-0105.
- **How placement is shown:** an inlay hint at each allocation ("region: `update`", "heap:
  popped elements leave `queue`", §14.6) and `dz placements <dir>`, as text or with
  `--format=json`, both read from the one pass that decides, as the clause hint is read off
  `DZ0415`'s fix, so they cannot disagree. Placement within a module is decided per module,
  so the editor can run it.
- **From [T-0099](T-0099-allocators-temp-gives-growing-container-malloc.md), dropped:** the
  region arena's chunks double, so a growing container does not cost a `malloc` per resize.
  The prototype's `ScopeArena` did this.
