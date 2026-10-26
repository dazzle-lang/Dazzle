---
id: T-0104
title: Prototype: the compiler chooses where memory comes from, by ownership
milestone: none
status: open
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
