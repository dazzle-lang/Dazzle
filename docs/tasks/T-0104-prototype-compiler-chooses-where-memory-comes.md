---
id: T-0104
title: Prototype: the compiler chooses where memory comes from, by ownership
milestone: none
status: done
done: r81, 2026-10-27
kind: design
area: ownership
where: `dz_flow` (the facts), `dz_cpp` (the placement), the `std` containers' `allocator` parameters
---

An idea the user raised on 2026-10-26, written down to be tried in a throwaway prototype
before anything is decided. The user doubts it is feasible; the prototype is how to find out.

**What it is.** Today a program picks an allocator by hand for every container (§14,
§18.10, D-0047), and two models sit side by side: an owned value is dropped at the end of
its owner's scope (§6.5), and `Allocators::Temp` frees everything at `reset()`. They clash in
two ways:

- A value in `Temp` still runs its drop, which ends in a `free` that does nothing. For a
  `List<String>` whose strings are in `Temp` too, that is a walk over every element to make
  n calls that do nothing.
- A value in `Temp` that outlives a `reset()` is dropped later on released memory. A
  `String` survives by luck; a `List<String>` reads its elements from a freed chunk and
  calls through a garbage allocator pointer.

Skipping drops for values in `Temp` is not the fix: the allocator of a container says
nothing about what its elements own (`List<String>(0, Allocators::Temp)` holding strings
from `String::from`, which uses `Global`, would leak every frame; a `List<File>` would never
close its files).

The idea is to **keep the semantics and make placement the compiler's job.** "The owner
drops the value at the end of its scope" stays exactly as it is. What changes is who
decides where the memory comes from. Single ownership already tells the compiler, without
running anything, where every value is created, moved and dropped: `dz_flow` computes it.
The same facts say whether a value can live in a cheaper place than the heap:

```dz
fn b() -> List<i32> { let l = List<i32>(); …; return l }   // l is built where the caller says

fn a() -> i32 {
    let xs = b()                  // never leaves a: a's region, released when a returns
    return xs.len() as i32
}

fn keep(&mut self) {
    self.items = b()              // outlives the call: the caller asks for the heap
}
```

- **A value that never leaves its scope** gets its memory from that scope's **region**, an
  arena released in one step when the scope exits.
- **A drop that only frees memory in that same region is skipped**: no walk over a
  `List<String>` whose strings are in the region too. A `File` or an `Rc` is still dropped.
- **A returned value is built where the caller says**, through a hidden parameter filled in
  at the call site, as `loc = #location` already is (what Zig calls result location). The
  callee is compiled once, and the caller chooses based on what it does with the result.
- **Per-frame scratch falls out of it**: what `update()` builds and drops lives in
  `update()`'s region, with no `reset()` to call and nothing that can outlive it.
- **Anything that cannot be placed goes to the heap**, as today.

**Why it matters.** It would remove both clashes above, take the allocator out of ordinary
code, and make the fast choice the automatic one, which T-0100 asks for. "Put scratch in
`Temp`" is advice that can backfire today (T-0099), and every program has to guess.

Three properties make it safer than it sounds:

- **It is an optimisation, not a rule.** No program's meaning changes, so correctness never
  depends on it, and a value it cannot place simply stays on the heap.
- **Region memory goes back at or after the owner's drop, never before**, so nothing dangles
  that does not dangle today.
- **It changes no diagnostic**, so it belongs to `dz build` alone. The LSP never runs it, and
  the whole-program work it needs sits beside `dz_mono`'s, which keeps invariant 3 for
  everything the editor uses.

**Why it is this way.** Nothing has been tried; this is the idea as discussed. The hard
parts, each of which the prototype has to answer:

1. **Allocating in a loop for an owner outside the loop**: `s = format!(…)` in a loop, or
   `self.items = List()` every frame. Putting those in the outer region grows memory without
   bound. This is where region inference has failed before: the ML Kit (Tofte–Talpin) needed
   further analyses and in the end a garbage collector. The rule has to be that an
   allocation site that can run more than once per lifetime of its region uses the heap,
   unless the value dies within the same iteration.
2. **Growing containers**: a list that doubles inside a region leaves its outgrown buffers
   behind until the region ends. That is at most about twice the memory in total, but in a
   long-lived region the heap is the better choice.
3. **Values passed into calls**: `list.push(String::from(x))` should put the string in the
   list's region, which needs to know that `push` stores its argument into `self`. That is a
   summary per function of where each parameter goes: written by hand for `std`, inferred
   for user code across the whole program, so a change to one function's body can change the
   code generated for its callers.
4. **What stays on the heap**: `Rc` and `Box` contents, anything reaching a field of
   something long-lived, anything sent to another thread, and pointers handed to C.
5. **Visibility.** §14.1 keeps the allocator explicit so a call shows whether it allocates
   and from where (principle 1, §1). Without the parameter, the compiler's choice has to be
   shown instead: an LSP inlay hint on each allocation site ("region: `update`", "heap:
   stored into `self.items` at line 40") and a `dz` command that lists them.

**What it would take.** First a prototype, kept off `master` or behind a flag, in stages,
each measured with `python tools/bench_programs.py` (T-0090) against today's code and
against hand-placed `Allocators::Temp`:

1. **Values that never leave their scope**, placed in a scope region, plus skipping drops
   that only free memory in that region. The biggest win for the least analysis.
2. **Caller-chosen placement for returned values**, through the hidden parameter.
3. **Where parameters go**: the per-function summaries, and the inlay hints.

Stages 1 and 2 alone cover the `a()`/`b()` case and per-frame scratch. Stage 3 is where most
of the effort and the risk are. The prototype answers **go** if stage 1 and 2 programs are
faster than or as fast as hand-placed `Temp`, peak memory stays near the heap's, and the loop
rule in point 1 can be stated simply enough to explain in an inlay hint. It answers **no-go**
if the loop and growth cases force so much onto the heap that the regions rarely apply, or
if the rules cannot be explained.

**The decision the user has to make**, after the prototype and only then: whether placement
becomes the compiler's choice. That re-decides §18.10 and D-0047, so it is a language change
with a decision file and a §21 row. The open sub-question is whether an explicit allocator
stays as an override (a container given one uses it; one given none is placed by the
compiler) or goes away. Until it is decided, the allocator stays explicit and T-0099 stands
on its own.

Related: [T-0099](T-0099-allocators-temp-gives-growing-container-malloc.md) (the arena's
growth cost) and [T-0100](T-0100-choose-temporary-arena-where-it-faster.md) (choosing the
arena automatically), of which this is a fifth option.

**Stage 1, prototyped on 2026-10-26** on the branch `t-0104-regions-prototype`, behind
`DZ_REGIONS=1` (`DZ_REGIONS_EXPLAIN=1` prints every decision, which is what an inlay hint
would show). What it does:

- `crates/dz_cpp/src/regions.rs` places `let x = List<T>(…)` — `String`, `HashMap`,
  `HashSet` and `String::from` likewise — when the allocator is left to its default and `x`
  is only ever borrowed afterwards: `&x`, a `std` method on it, `x[i]`, a slice, a `for`, or
  an assignment to it. Anything else keeps the heap.
- The region is a `ScopeArena` (`std/memory/scope_arena.dz`), declared first in the block
  that holds the `let`, so C++ releases it after everything in it is dropped. A block in a
  loop gets a fresh one per pass. Chunks come from a per-thread `ScopePool` and go back to
  it, so a region costs no `malloc` once the pool is warm, and each new chunk in a region is
  twice the last (T-0099's fix, applied here only).
- One hole needed the library: a function lent `&List<T>` can copy it into something
  long-lived, and a copy takes the original's allocator. `Allocator` gained `scoped()`, and
  a `List` or `HashMap` copied from a scoped allocator goes to the heap. `tests/run/regions`
  checks this (switching the guard off makes it read `-1 -1` from a reused chunk).
- Every `tests/run` program prints its expected output with regions on, as does
  `tests/run/regions` (`regions_change_nothing` in `crates/dz/tests/run.rs`). `cmake` fails
  on this machine either way, for want of Ninja.

The A/B, Windows, release, ten runs each. *hand* is the benchmark as written, *heap* the
same with every `Allocators::Temp` taken out, *regions* the heap version with
`DZ_REGIONS=1`:

| program | compiler | hand | heap | regions | peak MiB (hand / heap / regions) |
|---|---|---:|---:|---:|---|
| astar | clang++ | 699 ms | 700 ms | **578 ms (−17%)** | 22 / 22 / 29 |
| astar | g++ | 564 ms | 566 ms | **484 ms (−15%)** | 22 / 22 / 29 |
| expr | clang++ | 332 ms | 371 ms | 370 ms | 143 / 143 / 143 |
| expr | g++ | 303 ms | 336 ms | 337 ms | 143 / 143 / 143 |
| words | clang++ | 369 ms | 398 ms | 400 ms | 73 / 46 / 60 |
| words | g++ | 319 ms | 344 ms | 345 ms | 73 / 46 / 60 |

(`astar` uses no `Temp`, so its hand and heap builds are the same program.)

What it says:

- **Where the analysis reaches the hot allocations, it beats hand placement.** `astar`'s
  maps and path are locals of a function called once per search: placed, they are 15–17%
  faster than the heap and than `Temp`, which T-0099 measured as *slower* than the heap.
  The cost is memory: about a third more at peak, from outgrown map arrays kept until the
  region ends and the pool holding chunks.
- **`words` and `expr` are untouched, as stage 1 predicted.** In `words` the bulk is the
  `String` keys, copies of `word`, which is passed to `insert` by value and so stays on the
  heap. Putting it in the map's region needs to know that `insert` stores its argument:
  stage 3. In `expr` the allocations are `Box` nodes made by the program's own `Parser`,
  which the stage-1 rules do not place at all. Hand-placed `Temp` stays 7–11% ahead on
  both.
- **Against the go/no-go above:** stage 1 matches or beats hand-placed `Temp` where it
  applies, but peak memory is not near the heap's, and on two of the three programs it does
  not yet apply. Neither a go nor a no-go. The next things to learn are whether stage 3 (where
  parameters go) reaches `words`'s keys, and whether releasing outgrown buffers back into the
  region (a free that is not a no-op for the last allocation, or a size-classed free list)
  brings the peak down.

Not checked: tracking (`--track-memory`) knows nothing of regions, and `DZ_REGIONS_EXPLAIN`
is read inside codegen, which a real version would take from the build options.

**Stage 2, prototyped on 2026-10-26** on the same branch:

- **A result allocator.** A non-generic free function that returns something built by a
  call left to the compiler — `let xs = List<T>(); …; return xs`, or `return make()` — gets
  a hidden last parameter its signature does not show. Each call passes the caller's region
  (the result kept and only borrowed), the caller's own result allocator (the result returned
  again), or the heap. Decided once per function for the whole program; a function also used
  as a value, a generic one and a method get none.
- **`#allocator`**, an intrinsic like `#location`, as the default of an ordinary parameter:
  `allocator: ^mut Allocator = #allocator`. Filled in at each call by the same rules; the
  body passes it on explicitly. `std`'s containers, `String` and `Path` take it instead of
  `Allocators::Global`; `Box` and `Rc` do not, since a `Weak` can outlive the `Rc` it was
  made from. An early version also handed a function's `#allocator` to what it returned
  without the body naming it, and silenced the unused-parameter warning to allow it; the
  user rejected that, and the warning is back.
- `let x = make() or …` is followed through the `or`.
- Everything in `tests/run`, `tests/run/regions` and the crates' tests passes with regions on
  and off, apart from `cmake` and the tolerant-analysis fixture built on it, which need
  Ninja on this machine.

What the memory side taught, measured as above (*heap* and *regions* builds of the
heap-only programs):

| `ScopeArena` keeps outgrown buffers | astar | expr peak | words peak |
|---|---|---|---|
| in the region until it ends (stage 1) | −15–17%, +33% memory | +70% with stage 2 | +63% with stage 2 |
| no: blocks ≥ 16 KiB from `malloc`, freed at once | ±0–6% | as the heap | as the heap |
| no: blocks ≥ 16 KiB pooled by size class across regions | −15–17%, +7 MiB | +68% | +20% |
| as above, classes past 4 MiB not pooled | not measured (stopped) | | |

So **`astar`'s gain was never the pointer bump**: it was reusing the same memory across the
calls of one function, which the heap does not do. Keeping that and the heap's peak at once
is the open question for the memory side.

**What stage 2 showed about the design**, in the user's questions: a parameter cannot say
which of the values a function builds it is for. A function given `&mut List<String>` that
builds a scratch string and pushes another needs two different places, and one `#allocator`
gives both the same. That moves the work to
[T-0105](T-0105-placement-follows-value-destination-not-function.md) (placement follows
each value's destination, including into a container through `push`) and the language
question to [T-0106](T-0106-keep-manual-allocators-only-at-construction.md) (keep manual
allocators only at construction, and drop `#allocator`).

**Stage 3**, values stored into containers, was prototyped on 2026-10-27: its rules and
measurements are in T-0105.

**The design that grew out of this prototype** is [T-0107](T-0107-design-ties-kills-clauses-compiler-placed.md)
(decision [D-0091](../decisions/D-0091-ties-checked-reference-invalidation-memory-placed.md), open).

**Done, 2026-10-27:** the prototype answered its question. The user decided D-0091 (r81): placement becomes the compiler's, made safe by ties, and the remaining work is T-0107.
