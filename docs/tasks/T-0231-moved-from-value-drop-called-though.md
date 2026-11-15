---
id: T-0231
title: A moved-from value's drop is called though flow knows it is empty
milestone: M15
status: open
kind: feature
area: codegen
where: `crates/dz_cpp/src/lib.rs`: where a local's or temporary's C++ destructor runs at scope end; `crates/dz_flow/src/lib.rs`: the move analysis that knows a place was moved from; `crates/dz_project/src/std/memory/box.dz`: `Box`'s drop
---

Found 2026-11-14 measuring T-0229 (a one-word `Box`) on the `expr` benchmark. A parser moves
each node up through every level, and every move leaves a moved-from `Box` or `Expr` behind,
reset to its defaults, whose drop then runs and finds it empty. Ownership analysis knows
statically that the value was moved from, but the generated C++ still calls its destructor.

**Why it matters now.** Before T-0229, `Box`'s drop reached the value's drop through a
function pointer, so `~Box` was a small leaf and both compilers inlined the empty-box test
everywhere. Now it destroys the value directly, which drops its boxes, and a compiler
inlines nothing into a recursive call: clang++'s `expr` binary has 43 out-of-line calls to
`~Box`, 4.7% of the run, which took back what the smaller nodes saved (`expr` 137 -> 140 ms
on clang++, 134 -> 127 on g++). Splitting the drop into an inline null test and a function
of its own moved the cost to `Expr`'s drop on g++ instead (clang++ 129.5 ms, g++ 134.5), so
which compiler wins was inlining luck, and that split was not kept (`docs/log/tooling.md`,
T-0229's entry).

**What would fix it for every type, not only `Box`.** Where flow proves a place was moved
from on every path to the end of its scope, emit no destructor call for it: in C++, a local
moved from is a candidate for a scope that ends without its destructor, which needs the
local held in storage C++ does not destroy (a union member, or `std::optional`-like raw
storage) or the move lowered so nothing is left behind. `move(x)` of a copyable local
already skips the reset where it cannot be seen (T-0225's progress); this is the next step.
Measure on `expr` and `json` first, both compilers.

**2026-11-15, from T-0234.** `json`'s convert phase shows the same cost: each record is
built in a struct literal, moved into a `Res<Record>`, and moved into the list, and
`List<String>`'s destructor is called five times a record on values already moved from,
500,000 calls that free nothing. T-0234 removed one of those moves where the value goes
through a by-value parameter the body moves on (`moved_out_params`), under
`drops_silently`, the same guard this task would need; the bare convert loop is still
4–5 ms behind its C++ half's 11.

## Where the code is

| What | Where |
|---|---|
| `Box`'s drop, the recursion | `crates/dz_project/src/std/memory/box.dz` |
| A move's reset skipped where unseen (the precedent) | `crates/dz_cpp/src/lib.rs`, `drops_silently` and the `move(x)` lowering |
| What flow knows about moved places | `crates/dz_flow/src/lib.rs` |
