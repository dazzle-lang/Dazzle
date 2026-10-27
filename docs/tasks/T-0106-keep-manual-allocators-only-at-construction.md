---
id: T-0106
title: Keep manual allocators only at construction; drop #allocator parameters
milestone: none
status: done
done: r81, 2026-10-27
kind: design
area: ownership
where: spec §14, D-0047, §18.10; `#allocator` on the branch `t-0104-regions-prototype`
---

**What it is.** The regions prototype ([T-0104](T-0104-prototype-compiler-chooses-where-memory-comes.md))
tried `allocator: ^mut Allocator = #allocator`, a parameter the compiler fills in at each
call, as `#location` is. It works when a function builds exactly one thing, its result —
which is what a constructor does — but a parameter cannot say *which* of the values a
function builds it is meant for ([T-0105](T-0105-placement-follows-value-destination-not-function.md)).
The question this leaves is how much manual control the language keeps:

1. **No allocators at all.** The compiler places everything (T-0104, T-0105).
2. **An allocator named only where a value is built**, as today:
   `List<T>(0, &level_arena)`. Anything not given one is placed by the compiler. A function
   that wants its caller to choose takes an ordinary `^mut Allocator`, with no default, and
   passes it on itself; or it takes a `&mut List<T>` its caller built. No `#allocator`.
3. **Option 2 plus `#allocator` defaults**, as prototyped.

**Why it matters.** Option 1 removes a class of bugs — a value used after `Temp.reset()`,
memory a function's parameter put in the wrong place — and lets the compiler guarantee that a
region is released only after the values in it are dropped. But it gives up what engines and
embedded code rely on:

- frame data that `update()` stores in `self` and `draw()` reads, freed once a frame;
- a level arena freed all at once when the level unloads;
- fixed buffers and no `malloc` at all on an embedded target;
- a memory budget per subsystem.

None of these is visible to an analysis of one scope, so the compiler would put them on the
heap.

**Why it is this way.** Spec §14.1 keeps the allocator explicit so a call shows whether it
allocates and from where (principle 1). The prototype showed that "explicit" is clear only
where a value is built; a function parameter defaulted by the compiler is neither explicit
nor tied to a value.

**What it would take.** The user's decision, and a language change whichever way it goes:
a decision file superseding parts of D-0047 and §18.10, and a §21 row. The recommendation
from 2026-10-26 is option 2: the compiler's placement by default, an explicit allocator at
construction as the override, and `#allocator` removed from the prototype.

**Since 2026-10-27** the question is part of [D-0091](../decisions/D-0091-ties-checked-reference-invalidation-memory-placed.md):
the user now leans to option 1 (no allocator in ordinary code), made safe by the checker
[T-0107](T-0107-design-ties-kills-clauses-compiler-placed.md) describes, rather than option 2.

**Done, 2026-10-27:** the user chose option 1, no allocators in ordinary code, in D-0091 (r81).
