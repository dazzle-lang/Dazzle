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

**Progress, 2026-11-16: unions.** Measured with callgrind, `expr`'s `~Box` was 7.5% of its
instructions on clang++ (106 M), almost all of it 10.6 million calls on boxes that were
empty: a call, a null test and a return each. They came from three places, each a drop of a
value already moved from or never filled:

- **The variant literal.** `Expr::Binary{ … }` built the value at its defaults, switched
  variant (`become`), which built a `Binary` of two empty boxes, and assigned each field a
  temporary `Box` that was then dropped empty. A non-trivial union's literal now builds the
  payload in place, every field at once by position: `Expr e(no_payload{}, Kind::Binary);
  new (&e.Binary) Expr::Binary_t{op, Box(…), Box(…)}`, with no temporaries (`union_literal`).
  A payload literal names every field (a payload field's default is T-0237), so this is
  every such literal.
- **A move of a union.** The source kept its variant, a `Binary` of two empty boxes, so its
  drop, the `Res<Expr>`'s, `Box`'s parameter's and the assigned-over `lhs`'s, dropped
  them. A move now leaves the source holding the first variant, built over the moved-from
  payload without dropping it: a moved-from payload's drop does nothing (§6.4), so its
  storage can be reused. Only where the first variant is plain data and the union's drops
  are silent (`drops_silently`), so no `drop` the program wrote could see one skipped
  (`move_from` in `union_decl`). T-0097 had stopped resetting the source because the reset
  then was a switch, a drop and a construct on every move; this one is two stores.

Instruction counts against T-0220's build (clang++ / g++): `expr` -11.0% / -0.4%, `json`
-4.6% / -1.2%, the others unchanged. Time on `expr` with clang++ 5.9% less; with g++ 2% more
over 25 runs, though g++'s instructions fell and the generated `tokenize` is the same text:
g++ chose to inline `operator==` on text into `tokenize` this time, which its count shows.
`a_moved_union_is_left_plain_and_a_variant_is_built_in_place` pins both shapes, and
`tests/run/value_shapes` the values.

**Tried again and not kept:**

- **`Box`'s drop split** into an inline null test and an out-of-line `release`: -6.8% of
  `expr`'s instructions on clang++ but +4.1% on g++, which had inlined the whole drop and
  now called `release`; with `release` `#[inline]` too, +8.7% on g++. As T-0229 found.
- **A constructor taking every field** for a struct C++ does not call an aggregate
  (`json`'s `Record`, for its placed copy), so a literal of every field builds each member
  from its value rather than assigning a value built at its defaults: 0.4% of `json`'s
  instructions on clang++ and 0.1% on g++, for a template constructor in many headers.

**Left:** `json`'s convert loop still drops five moved-from `List<String>`s a record, through
its struct's drop rather than a union's, and still builds a `Res<Record>` for every return;
neither measured since.

| Piece | Where |
|---|---|
| A variant literal built in place | `union_literal` in `crates/dz_cpp/src/lib.rs`; `no_payload` in `runtime.rs` |
| A moved-from union left in its first variant | `move_from` in `union_decl`, `crates/dz_cpp/src/lib.rs` |
