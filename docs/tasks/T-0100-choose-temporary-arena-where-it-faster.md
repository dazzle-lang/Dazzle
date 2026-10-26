---
id: T-0100
title: Choose the temporary arena where it is faster, automatically or by a setting
milestone: none
status: open
kind: design
area: stdlib
where: `crates/dz_project/src/std/memory/allocators.dz`; the containers' `allocator` parameters
---

**What it is.** A program picks its allocator by hand, one container at a time:

```dz
let mut counts = HashMap<String, i32>(0, Allocators::Temp)   // scratch: the arena
let mut best = HashMap<Cell, i32>()                          // the default: the C heap
```

Whether the arena is the faster choice is not obvious from the source. In the whole-program
benchmarks (T-0090), it made `expr` and `words` 5–11% faster and `astar` about 5% slower,
for reasons only a measurement showed: `astar`'s maps grow past the arena's chunk size, and
each resize then costs a `malloc` of its own (T-0099). The user raised it on 2026-10-24:
it would be useful for the choice to be made automatically, or by a configuration flag,
rather than by every program guessing.

**Why it matters.** "Put scratch in `Temp`" is the advice the spec gives (§14.2). If
following it can make a program slower, people either stop following it or have to profile
every container.

**Why it is this way.** An allocator is a value passed to a constructor, so the choice is
made where the container is built, and nothing measures it.

**What it would take.** Design first; this is the user's decision. Some options:

1. **Make the arena never the slower choice**, which is T-0099 (geometric chunk growth),
   and then make "use `Temp` for scratch" safe advice. Library work only.
2. **A build setting** that redirects what `Allocators::Global` hands out, such as
   `[build] scratch = "temp"`, or one per scope. This is a language-level decision: it
   changes what a program's allocations mean, and it needs a D-NNNN and a §21 row.
3. **A scoped default**, where a block names the allocator its containers default to. This
   is a language change too, and the most explicit of the three.
4. **Automatic**, using a profile from a previous run to choose per allocation site. This
   is the most work, and it makes a program's behaviour depend on a profile.
5. **The compiler chooses where memory comes from**, from ownership alone, with no
   allocator in ordinary code. It is the most far-reaching option and needs a prototype
   first: [T-0104](T-0104-prototype-compiler-chooses-where-memory-comes.md).

Option 1 is worth doing whatever is chosen, and may make the rest unnecessary.
