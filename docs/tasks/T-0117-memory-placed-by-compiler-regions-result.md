---
id: T-0117
title: Memory placed by the compiler: regions and the result placement
milestone: none
status: done
kind: feature
area: ownership
---

Step 4 of D-0091's plan (spec §14.6), split out of
[T-0107](T-0107-design-ties-kills-clauses-compiler-placed.md), which has the full design and
the scenarios it was checked against.

**Where the code is** (keep this current; stage 1's rounds 1 and 2 and stage 2's round 1,
below, built it).

| Piece | Where |
|---|---|
| The region arena: `ScopeArena` (chunks doubling; a block of 16 KiB or more is the heap's, at its exact size, freed at once) and `ScopePool` (per thread; chunks only) | `dz_project/src/std/memory/scope_arena.dz`, `scope_pool.dz`, listed in `STD_SOURCES` (`dz_project/src/lib.rs`) |
| A copy is built in its own placement, never its original's (T-0140, which replaced the `Allocator::scoped` guard): `List::copy` and `HashMap::copy` read `#placement`; `String`, `Path`, `HashSet` and `SlotMap` copy field by field through them | `std/collections/list.dz`, `std/collections/hash_map.dz`; the placed copy in T-0140's table |
| Sites, the proposer and the reasons' words: `place`, `place_with` (`Proposer::Rules`, `Proposer::Everything`), `Placements`, `BodyPlacement` (`region_blocks`, `region_for`, `demoted`), `Site`, `Decision` | `dz_flow/src/placement.rs` |
| The ties check: `analyse_regions`; `Value::owned`, `Regions`, `Refusal`; `region_site` (the loop rule), `region_assign`, `region_end`, `sink`, `passed` (a copy carries none of its original's regions, only its own when placed) | `dz_flow/src/ties/body.rs` |
| One answer for every reader: `Analysis::placements`, computed after ownership and timed as `placement` | `dz_driver/src/lib.rs` |
| Codegen, behind the switch: `Options::regions`; `emit` takes `Option<&Placements>`; `Emitter::placement` and `regions`, set in `in_body` by the body's owner; the region declared in `block_into`, passed in `call` (`region`), named by `region_name` | `dz_cpp/src/lib.rs` |
| The switch: `DZ_REGIONS` (any value but `0`), for every command that emits C++ | `emit_cpp`, `crates/dz/src/build.rs` |
| One wording for every reader: `Decision::label` | `dz_flow/src/placement.rs` |
| Stage 2, result placement: `result_functions` (which take one; `holds_allocator`, `allocator_param`), `Placements::takes_result`, `callee_owner`, `Decision::Result`, `Placed`, `BodyPlacement::placed`; the proposer's `Target::Result`, `Escape`, `returned` | `dz_flow/src/placement.rs` |
| Stage 2 in the check: `Regions::results`, exempt from `region_end` and from the `return` refusal | `dz_flow/src/ties/body.rs` |
| Stage 2 in codegen: the hidden parameter in `decl_params` (`RESULT_PLACEMENT`, `allocator_ty`), the argument in `call` (`placement_arg`, `heap_placement`, `call_takes_result`) | `dz_cpp/src/lib.rs` |
| Stage 2, round 2: trait methods by the trait's signature and function types by their result (`result_functions`, `keeps_allocator`, `owner_sig`; `callable` on `Callee::Value`); results followed through `or` (`unwrapped`, `returned`) | `dz_flow/src/placement.rs` |
| Stage 2, round 2 in codegen: a function type's parameter (`cpp_ty`, `Ty::Fn`), a vtable slot's (`dyn_signature`), `passes_result`, and the adapter for a function value whose function takes none (`result_adapter`) | `dz_cpp/src/lib.rs` |
| Stage 2, round 3: what holds a value places it (`fate`, `holder`, `holds`, `escape_of` following a `let`, `temporary`); `boxes_argument`; constructors in `result_functions`, `self_field` | `dz_flow/src/placement.rs` |
| Stage 2, round 3 in the check: a `Box`'s contents carried into its value (`contents`, in `call` and the `Call` node); a constructor's result site stored into a field of `self`, then held by `self` (`region_assign`, `self_local`) | `dz_flow/src/ties/body.rs` |
| Stage 2, round 3 in codegen: a constructor's heap overload, declared and delegating (`heap_ctor`, in `struct_decl` and `struct_bodies`) | `dz_cpp/src/lib.rs` |
| `format!`'s text placed where the macro's value goes (`formatted_text`, in `propose`) | `dz_flow/src/placement.rs` |
| A by-value parameter that stays in the call (`Staying`, `staying_params`, `owner_module`; `fate` on a call's argument); the check's side (`analyse_regions`' `params`, `Regions::params`, `stays_in_call`) | `dz_flow/src/placement.rs`, `dz_flow/src/ties/body.rs` |
| The inlay hint: `HintKind::Placement`, `placement_hints`; sent, with its tooltip, by `inlay_hints` | `dz_lsp/src/ide.rs`, `dz_lsp/src/lib.rs` |
| `dz placements [path] [--format=json]`: `cmd_placements` | `crates/dz/src/main.rs` |
| Tests | `dz_flow/tests/placement.rs` (stage 2: `a_returned_value_is_built_where_the_caller_keeps_it`; `a_formatted_string_goes_where_the_macro_value_goes`, `a_parameter_that_stays_in_the_call_takes_the_callers_region`); `tests/run/region_arena` (the arena by hand and the copy guard, also under `--track-memory` and `-Wall -Wextra`); `tests/run/regions` and `regions_change_nothing` (every run program with regions on, warning-clean on clang++ and g++) and `dz_placements_prints_each_site` in `dz/tests/run.rs`; `inlay_hints_show_where_each_allocation_goes` in `dz_lsp/tests/ide.rs` |
| The gate's stopwatch | `tools/bench_regions.py` |

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
anything the compiler cannot see, the default is the heap
([T-0142](T-0142-by-value-parameter-placement-inferred-only.md)). With the ties checker, a wrong
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

**Settled 2026-11-01: the gate is split in two.** Regions are not finished after stage 1,
so stage 1 is not judged as if they were. Stage 1 does not reach the hot allocations of
`words` (its map keys, stored where the map lives) or `expr` (its `Box` nodes, returned
from its own `Parser`); T-0104 measured them unchanged, 400 ms against 398 and 370
against 371.

| When | What it is | Passes when |
|---|---|---|
| after stage 1 | **a checkpoint**: a failure is a bug to fix, not a reason to drop regions | `astar` faster than the heap; `words` and `expr` not slower (a median at most 2% above the heap's, `NOISE` in `tools/bench_regions.py`); the loop flat; peak memory within 10%; every wrong placement refused |
| after stage 2 (result placement) | **the verdict**: keep regions or drop them, as above | every row of the table above, `astar`, `words` and `expr` each faster than the heap |

`words` may need more than stage 2: its keys are copies stored into the map, and reaching
them means building a stored value where its container lives (T-0105's "placement
follows the value's destination"), which neither stage has yet.

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
Committed on the branch `t-0104-regions-prototype` (`3b82370`). No program ran differently yet:
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

**Stage 1, round 2, built 2026-11-01: codegen behind the switch, and each placement shown.**

- **Codegen.** `dz_cpp::Options::regions`, set by `emit_cpp` from `DZ_REGIONS` (any value
  but `0`) for every command that emits C++, so `tools/bench_regions.py` builds one source
  both ways. A block with a region declares `ScopeArena _dz_regionN{};` first, and a placed
  call passes `::dz::addr(_dz_regionN)` in place of its default allocator. A body's
  placements are found by its owner, which `dz_mono`'s instances share with their generic
  body. With the switch off nothing changes.
- **Round 1's codegen edges hold**: `List`'s C++ copy constructor and copy assignment both
  call `List::copy`, which every other std container copies through, and codegen turns no
  last use into a C++ move.
- **Shown** by an inlay hint after each site and by `dz placements [path]` (text, or
  `--format=json`), both through `Decision::label`. The command lists the project's own
  files only; `std`'s sites are decided but not listed.
- **Checked**: `regions_change_nothing`, every `tests/run` program with regions on against
  its `expected.txt`, and the region C++ of the `WARNING_CLEAN` fixtures compiled
  `-Wall -Wextra` on clang++ and g++; `tests/run/regions` (the prototype's cases less stage
  2) places 13 of its 24 sites and prints the same both ways. The rest of `dz/tests/run.rs`
  passes but `every_example_builds`, for want of the vendor submodules. The `dz_lsp`,
  `dz_flow`, `dz_driver` and `dz_cpp` tests, clippy with `-D warnings`, and `cargo fmt`.

**Stage 1, round 3, the checkpoint, measured 2026-11-01** at commit `bc1e1f7`:
`python tools/bench_regions.py`, `--release`, the median of ten alternating runs of each
build. Intel Core (family 6, model 158, 8 threads), Windows 10; clang 21.1.8 and MSYS2
g++ 13.1.0. Peak memory is the peak working set.

| Program | Compiler | Heap | Regions | Regions / heap | Peak, heap / regions | Faster | Not slower (2%) | Peak within 10% |
|---|---|---:|---:|---:|---:|---|---|---|
| astar | clang++ | 987 ms | 862 ms | 0.873 | 21.3 / 27.8 MiB | yes | yes | **no** (+31%) |
| astar | g++ | 788 ms | 632 ms | 0.803 | 21.4 / 28.2 MiB | yes | yes | **no** (+32%) |
| words | clang++ | 736 ms | 720 ms | 0.978 | 45.3 / 50.9 MiB | yes | yes | **no** (+12%) |
| words | g++ | 653 ms | 665 ms | 1.019 | 45.7 / 51.3 MiB | no | yes | **no** (+12%) |
| expr | clang++ | 628 ms | 628 ms | 1.001 | 142.3 / 142.1 MiB | no | yes | yes |
| expr | g++ | 563 ms | 566 ms | 1.006 | 142.7 / 142.7 MiB | no | yes | yes |

| Loop rule | Compiler | Heap: peak at 10,000 / 1,000,000 | Regions: the same | Flat |
|---|---|---:|---:|---|
| reassigned in a loop | clang++ | 6.3 / 5.8 MiB | 6.3 / 5.9 MiB | yes |
| reassigned in a loop | g++ | 6.2 / 6.6 MiB | 6.3 / 6.6 MiB | yes |

Every wrong placement is refused: `the_region_check_alone_refuses_every_wrong_placement`
and the other five tests in `dz_flow/tests/placement.rs` pass.

**Read against the checkpoint:** speed passes (`astar` 13% and 20% faster; `words` and
`expr` within 2%), the loop is flat, and safety holds. **Peak memory fails**: `astar` +31%,
`words` +12%, against 10%. The prototype saw the same, "a third more peak memory" for
`astar` (T-0104). By the checkpoint's terms this is a bug to fix, not a reason to drop
regions.

**The likely cause, not yet confirmed:** `ScopePool` keeps every chunk and every freed
large block a region gives back, in every size class, with no limit. A map that doubles
from 16 KiB to its final size hands each outgrown array to the pool, so the pool ends up
holding about as much again as the map's final array. `astar` has two such maps, and
6.5 MiB extra is about their size. Smaller outgrown arrays also stay in their chunk until
the region ends, since only the last allocation can be given back. To confirm: measure
what the pool holds at `astar`'s peak. Possible fixes, smallest first: keep at most one
block per size class; free a block to the heap when the pool already holds one of its
class; or give large blocks straight back to the heap and pool only chunks.

**The peak-memory bug, fixed 2026-11-02.** Stage 1 is done; the keep-or-drop verdict
waits for stage 2.

- **The cause, confirmed** by taking the pool out of the large-block path alone: with
  every large block straight from the heap at its exact size, peak memory fell to the
  heap's in all three programs (`astar` 20.0 against 19.9 MiB, `words` 45.5 against 44.0,
  `expr` 141.2 against 141.1). There were two costs. The pool kept every class a container
  grew through, as guessed above. And it rounded each block up to a power of two plus its
  header, which is why `expr` was 8 MiB over although its sizes do not repeat.
- **A byte budget does not save the pool.** Held to 512 KiB, 1, 2 or 4 MiB a thread, it
  kept the extra memory, from 0.7 to 3.4 MiB on `astar` and the full 8 MiB on `expr`, and
  lost the speed: `astar` from 0.98 to 1.03 of the heap. The 3–7% it had bought came only
  with the unbounded pool, so the speed was paid for in memory.
- **The fix** is the last of the three above: a large block is the heap's, at its exact
  size, and goes back to it when freed. `ScopePool` keeps chunks only, and a large block's
  header is its two links. This is what the heap does too: `Heap::realloc` is also allocate,
  copy, free.
- **Found on the way**: the heap overload of a constructor that takes a placement (round 3)
  cast a by-reference parameter as `static_cast<Rng&&&>`, so `astar`, whose `Grid` takes
  `&mut Rng`, did not compile with regions on. `result_adapter` had the same cast. Both
  now pass a reference on as it is (`forward` in `dz_cpp`), and `tests/run/regions` has a
  constructor taking `&List<i32>`.

The checkpoint again, at `7fdd285` plus this fix: `python tools/bench_regions.py`,
`--release`, the median of ten alternating runs. AMD Ryzen 7 H 255 (16 threads), Linux,
glibc 2.44; clang 23.1.4 and g++ 16.2.1. Peak memory is the peak resident set. The machine
was busy (load 3 to 6), and the heap's own medians moved by up to 20% between sweeps, so
read the times as roughly even.

| Program | Compiler | Heap | Regions | Regions / heap | Peak, heap / regions | Faster | Not slower (2%) | Peak within 10% |
|---|---|---:|---:|---:|---:|---|---|---|
| astar | clang++ | 719 ms | 722 ms | 1.004 | 19.8 / 20.0 MiB | no | yes | yes |
| astar | g++ | 578 ms | 567 ms | 0.981 | 19.8 / 20.0 MiB | yes | yes | yes |
| words | clang++ | 462 ms | 464 ms | 1.003 | 44.7 / 46.3 MiB | no | yes | yes |
| words | g++ | 587 ms | 459 ms | 0.782 | 44.6 / 46.0 MiB | yes | yes | yes |
| expr | clang++ | 389 ms | 401 ms | 1.029 | 141.3 / 141.3 MiB | no | **no** | yes |
| expr | g++ | 312 ms | 306 ms | 0.982 | 141.3 / 141.3 MiB | yes | yes | yes |

The loop stays flat: 4.0 MiB at ten thousand passes and at a million, both ways, on both
compilers.

**Read against the checkpoint:** peak memory now passes, within 1% of the heap on `astar`,
4% on `words`, and even on `expr`. The loop is flat. Speed no longer passes: `astar` is
even with the heap, where the checkpoint asks for faster. On this machine its gain had
been the pool's (0.97 and 0.93 with it, measured the same day), not the region's. Most of
its allocations are its maps' arrays, which are large blocks, and large blocks are now the
heap's. `expr` on clang++ is 0.9% over the noise line, on a busy machine. Stage 2 has to
earn the speed at the verdict, through what it places in chunks, not through holding
memory.

**What placement rests on (found 2026-11-01).** A placement is checked like a reference, so
it is only as sound as the ties check, and the review behind
[D-0094](../decisions/D-0094-safe-code-has-no-undefined-behaviour.md) found holes in that
check: [T-0123](T-0123-call-may-reach-memory-it-kills.md),
[T-0124](T-0124-two-rc-may-hold-one-value.md),
[T-0125](T-0125-box-emptied-while-if-let-holds.md) and
[T-0127](T-0127-view-struct-drop-not-counted-as.md). Close them before the verdict is
trusted. [T-0131](T-0131-memory-tracking-poisons-every-freed-block.md) makes a wrong
placement show in tests, and [T-0132](T-0132-sanitizer-builds-in-ci-run-corpus.md) runs the
corpus with regions on under AddressSanitizer.

**Stage 2 plan (2026-11-01), the result placement, in three rounds.** The user put
benchmarks aside on a busy machine: what matters is that placement is automatic, checked,
and right.

1. **Round 1: free functions and methods.** A *result function* takes a hidden last
   parameter, `_dz_result`, the `^mut Allocator` its result is built in. It is one whose
   return type owns memory (looking through `?`), which is a free function or a struct or
   union method that is not `virtual` or `override`, has a body, takes no allocator
   parameter of its own (those are stage 1's sites), and is never used as a function value
   anywhere in the program. `Placements::takes_result` is the one answer, read by codegen
   for every declaration (`decl_params`) and every call.
   - **Callee side**: a site whose value is returned (`return List()`, `let xs = List();
     …; return move(xs)`, `return make()`) is decided `Decision::Result` and passes
     `_dz_result` on. The check treats it as a region on the body's outermost block that is
     never released: returning it is allowed, and storing it anywhere else, passing it by
     value, or making it on every pass of a loop is refused, as for a region.
   - **Caller side**: a call to a result function is a site like stage 1's, decided the
     same way: the caller's region when the result is kept and only borrowed, its own
     `_dz_result` when it is returned again, the heap otherwise. Every call passes one.
2. **Round 2: trait methods and function values.** Their C++ shape changes (a vtable
   slot, a function type), so the hidden parameter is part of the trait's and the function
   type's calling convention. A function called from C or C++ returns on the heap.
3. **Round 3: constructors and `copy`** build into wherever `self` is placed, so a value's
   owned parts live where it lives, and `Box`'s contents go where the `Box` goes.

The user's frame-region idea (one region per pass of an update loop, shared by the
functions it calls) needs the same hidden parameter, and is a design question for after
round 1: [T-0139](T-0139-frame-regions-one-region-per-pass.md).

**Stage 2, round 1, built 2026-11-01: free functions and methods.** Behind `DZ_REGIONS`,
as stage 1; with it unset no declaration or call changes.

- **Which functions take a placement** (`result_functions`): decided from signatures and
  the program's function values alone, so every caller and the declaration agree. The
  result must *keep an allocator* (`holds_allocator`): `List`, `String`, `HashMap` and what
  holds them, through fields and `?`. Needing a drop is not enough — `Formatter` and `File`
  need one and allocate nothing, and taking `needs_drop` put a placement on every
  `println!`. A bare type parameter does not count either: `pop`, `get` and `take` return
  a `T` that exists already, and an instance must not be called differently from another.
- **What is built there**: `return List()`, `return make()`, and `let xs = …` whose only
  escape is being returned. The check refuses the rest as for a region: stored outside the
  body, passed by value, or made on every pass of a loop (a value returned from inside a
  loop is made on every pass, so it keeps the heap).
- **What callers pass**: their region when they keep the result and only borrow it, their
  own `_dz_result` when they return it, the heap otherwise. In the benchmarks: `words`'
  generated text and `expr`'s generated source and token list are now built in `main`'s
  region, as is `Process::args()` in all three. `expr`'s parser nodes are not: each
  `self.expr() or return …` is kept through `or`, which keeps the heap.
- **Not yet** at round 1: trait methods, functions used as values, a result kept through
  `or` (all three done since, below); constructors and `copy` (round 3); and a temporary
  (`evens().len()`) or a value assigned rather than bound by `let`, which keep the heap as
  in stage 1.
- **Checked**: `regions_change_nothing` (every `tests/run` program, std's own result
  functions included, prints its `expected.txt` with regions on, warning-clean on clang++
  and g++); `a_returned_value_is_built_where_the_caller_keeps_it` (callee and caller
  sides, the loop refusal, and what takes no placement: a function value, a trait
  implementation, a generic `-> T?`); the corpus test, where the check refuses no result
  placement the proposer makes. `tests/run/regions` places 16 of its 29 sites, three of
  them in a result placement.
- **Missed at the commit (`daf89df`)**: the fixture's comments were edited after
  `dz_placements_prints_each_site` last ran, which moved its lines, so that test failed
  at that commit. Fixed in round 2's.

**Stage 2, round 1, follow-up (2026-11-01): a result kept through `or`.** `let x = make()
or …` and `return make() or …` are decided as `let x = make()` and `return make()` are;
the check already followed a value through `or`. In `expr` this places `primary`'s and
`unary`'s results where their callers keep them; most of its nodes then go into a `Box`
as a constructor argument, which is round 3's.

**Stage 2, round 2, built 2026-11-01: trait methods and function values.** The rule in
round 1 left both out; now nothing is left out but `virtual` methods.

- **A trait method** takes a placement when the *trait's* signature's result keeps an
  allocator, so all its implementations (their bodies and the defaults `dz_mono` copies
  into them), the trait object's vtable slot (`dyn_signature`), and a call through a
  bound or an object agree. A trait returning `Self` takes none, for any implementation.
- **A function type** whose result keeps an allocator passes one, `::dz::fn<R, P…,
  Allocator__dyn>`, and a call through a value is a site like any other. A function named
  as a value whose own declaration takes none — a generic `same<T>(x: T) -> T` named as
  `same<String>`, or a function with an allocator parameter of its own — is wrapped in a
  capture-less adapter that takes one and calls it as before (`result_adapter`).
- `tests/run/regions` gains a trait with a default method, called directly, through a
  bound and through a trait object, a function value passed and called, and the adapter;
  41 sites, 22 in a region. Building it found T-0121, a codegen bug of its own (a local
  hiding a free function of the same name), which fails with regions off as well.
- **Still out**: `virtual` and `override` methods, until a base and its overrides are
  decided together ([T-0141](T-0141-virtual-override-methods-take-no-result.md)); a function exported to C (`#[export]` is not implemented).

**Stage 2, round 3, built 2026-11-01: a value's owned parts live where it lives.**

- **What holds a value places it.** The proposer follows a site's value through
  everything that takes it whole — `move`, `or` (its operand or its fallback), a struct or
  union literal, a tuple, an array, the value a `Box` is made of — and into the local a
  `let` moves it to, then decides by where that ends up: a block's region, the caller's
  placement, or the heap (`fate`, `holder`). So `let p = Pair{ left: List(), … }` lent only
  puts both lists in the block's region, `return Pair{ … }` builds them where the caller
  keeps the pair, and `let first = List(); let p = Pair{ left: move(first), … }` follows
  `first` into `p`. A `move` is never a copy, even of a copyable `String`.
- **A `Box`'s contents** go where the `Box` goes: the check carries the regions of the
  value a `Box` is made of into the box's own value, as a struct literal does its fields',
  so every escape of the box refuses them both.
- **A constructor** whose struct has a field that keeps an allocator takes a placement
  for the value it makes, and builds there what it assigns to a field of `self`. The check
  lets a result site into a field of `self` in a constructor, and then holds it in `self`,
  so a later escape through the field is followed. A constructor cannot move a field out
  of `self`, nor change what its parameters reach (T-0122), so those escapes cannot be
  written. Each constructor taking a placement also gets a C++ overload without it, which
  delegates with the heap, for `let v: T` and a derived type's base initialiser.
- **`copy` is not done**, and is left for after the gate. A copy is made by C++'s copy
  constructor and copy assignment, which can take no placement, so placing one means a
  second, placed copy per type (`List`, `HashMap`, and every struct with a `copy` or with
  fields that have one), emitted where a copy is bound by `let`. Copies are correct as they
  are, on the heap (stage 1's step 3), so this is speed only:
  [T-0140](T-0140-copy-built-on-heap-wherever-it.md).
- **Not reached, by design**: `expr`'s nodes. Its `Box`es name the parser's allocator,
  which only T-0119 takes out of ordinary code, and each new node is assigned to `lhs`,
  which outlives the loop that builds it, so the loop rule sends it to the heap.
- **Checked**: `owned_parts_live_where_their_value_lives` and
  `a_constructor_builds_where_its_value_is_placed` in `dz_flow/tests/placement.rs`, each
  escape refused by the check alone too; `tests/run/regions` gains a struct with two
  constructors (one taking nothing, so `let v: T` runs the heap overload), a literal's
  fields and a box's contents, and prints the same both ways: 54 sites, 33 in a region.
  `astar` now places its priority queue as well as its two maps: `MinHeap`'s constructor
  builds its list where the queue is (`astar_places_its_queue_maps_and_path`).
- **Found**: T-0122, a constructor cannot declare a `kills` clause, so one that changes
  what a `&mut` parameter reaches cannot be written. A grammar question, the user's call.

`tools/bench_regions.py` builds the heap version of each benchmark (every
`Allocators::Temp` taken out, as T-0104 did) twice, without and with `DZ_REGIONS=1`, checks
that both print the same checksum, and reports the medians and peak memory; and a loop that
reassigns a `String`, a `List` and a `format!` result, whose peak at a million passes must
be within 1 MiB of its peak at ten thousand. Since round 2 codegen reads
the switch, so it measures regions. Since r104 (T-0119) the benchmarks name no allocator,
so each is built as written; `--hand-src` and `--hand-dz` add a third build, the program
before r104 with its scratch in `Temp`, from an older checkout.

**Stage 2, the verdict, measured 2026-11-02** at `c2f7a1c` (T-0118, T-0119 and T-0145 in):
`python tools/bench_regions.py`, `--release`, the median of ten alternating runs, with the
hand build from `39680ee` (before r104). Intel Xeon @ 2.80GHz (4 threads), a cloud VM, Linux,
glibc 2.39; clang 18.1.3 and g++ 13.3.0. Peak memory is the peak resident set. The load
average was about 1, but the VM was noisy: a build's median sat 10 to 40% above its
fastest run, so the medians move by more than the differences they are read for. The
fastest run of each build is given too.

| Program | Compiler | Heap | Regions | Regions / heap | Fastest, regions / heap | Peak, heap / regions | Faster | Not slower (2%) | Peak within 10% |
|---|---|---:|---:|---:|---:|---:|---|---|---|
| astar | clang++ | 1243 ms | 1245 ms | 1.002 | 0.957 | 19.3 / 19.4 MiB | **no** | yes | yes |
| astar | g++ | 973 ms | 937 ms | 0.964 | 1.013 | 19.2 / 19.4 MiB | yes | yes | yes |
| words | clang++ | 1018 ms | 872 ms | 0.856 | 1.003 | 43.0 / 44.9 MiB | yes | yes | yes |
| words | g++ | 1011 ms | 1053 ms | 1.041 | 0.961 | 43.0 / 44.9 MiB | **no** | **no** | yes |
| expr | clang++ | 860 ms | 776 ms | 0.902 | 0.993 | 139.0 / 139.1 MiB | yes | yes | yes |
| expr | g++ | 728 ms | 714 ms | 0.981 | 0.906 | 139.1 / 139.1 MiB | yes | yes | yes |

| Program | Compiler | Hand (`Temp`) | Regions | Regions / hand | Fastest, hand / regions | Peak, hand / regions |
|---|---|---:|---:|---:|---:|---:|
| astar | clang++ | 1265 ms | 1245 ms | 0.984 | 1133 / 1062 ms | 19.2 / 19.4 MiB |
| astar | g++ | 1003 ms | 937 ms | 0.935 | 897 / 887 ms | 19.2 / 19.4 MiB |
| words | clang++ | 744 ms | 872 ms | 1.172 | 682 / 714 ms | 69.2 / 44.9 MiB |
| words | g++ | 884 ms | 1053 ms | 1.191 | 790 / 829 ms | 69.2 / 44.9 MiB |
| expr | clang++ | 765 ms | 776 ms | 1.014 | 713 / 722 ms | 139.0 / 139.1 MiB |
| expr | g++ | 679 ms | 714 ms | 1.052 | 589 / 603 ms | 139.1 / 139.1 MiB |

The loop stays flat: 3.4 to 3.5 MiB at ten thousand passes and at a million, both ways, on
both compilers. Every wrong placement is refused: the 15 tests in
`dz_flow/tests/placement.rs` pass, `the_region_check_alone_refuses_every_wrong_placement`
and the corpus sweep among them.

**Read against the verdict:** peak memory, the loop and safety pass. Speed does not pass as
written: by the medians `astar` on clang++ is even and `words` on g++ 4% slower; by the
fastest runs the two builds are within 1% on four of six rows, and no row is outside the
noise. Regions are not slower than the heap and not reliably faster. Against hand
placement, `astar` is as fast, `expr` within noise, and `words` about 5% slower by its
fastest runs (17 to 19% by medians) while using 35% less memory; its keys are T-0149's
and T-0140's.

**Decided 2026-11-02 by the user: regions stay.** Memory, the loop and safety pass, and
speed is even with the heap within the noise, not worse. Regions were never to be judged
on stage 2 alone for `words` (above), and the speed rows are carried by what reaches the
rest of its allocations and `expr`'s:
[T-0149](T-0149-stored-values-left-on-heap-map.md) (a map filled in a loop) and
[T-0140](T-0140-copy-built-on-heap-wherever-it.md) (copies). Re-run
`tools/bench_regions.py` after each, on a quieter machine than this one. T-0118 and T-0119
are done; this task is done with the verdict.

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
