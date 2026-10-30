---
id: T-0116
title: Debug-build check of std's trusted kills clauses
milestone: none
status: open
kind: feature
area: ownership
---

Step 3 of D-0091's plan, split out of [T-0107](T-0107-design-ties-kills-clauses-compiler-placed.md).

**What it is.** `std`'s containers write their `kills` clauses by hand over `unsafe` code,
and the checker trusts them (§6.7). A wrong clause, `push` forgetting `kills self@items`,
would let every caller keep a reference into a moved array, silently. This adds generation
counters to container memory in debug builds: each allocation records a generation, a view
remembers the one it was made under, and using a view whose memory has since been killed
traps with the source location, so a wrong clause fails a test instead of shipping.

**Why it matters.** It is what makes the trusted part of the checker checked, in tests if not
at compile time, and with [T-0115](T-0115-moving-elements-within-container-array-kills.md)
it shrinks what is trusted. D-0093 lists it as one of the three things left unchecked by
design.

**Why it is this way.** The clauses cannot be checked statically without the analysis
understanding `unsafe` pointer arithmetic, which it deliberately does not.

**What it would take.** Compiler and runtime work (`docs/log/`), and tests that hand-break a
clause (`push` without `kills self@items`) and expect the trap.

**Settled in r90: a view carries nothing; the clause is checked at the function's edges.**
Generation counters carried by views would turn every C++ reference into a struct in debug
builds. Instead, in a debug build, a body the analysis trusts (`index_origins` marks it:
"a body over a tied pointer's elements is trusted") is checked where it starts and ends:

- **On entry** it records the memory under every tie of its reference parameters that its
  clause does not kill: each pointer field's address for a tie such as `@items`, and for
  `[*]` the blocks the elements own.
- **On each return** it compares. A difference traps, naming the function and the tie:
  "`List::push` killed `self@items`, which its clause does not name".

So views keep their layout, and release builds have none of it. `[*]` is checked exactly:
elements moved within the array keep their blocks, and one taken out loses its own. That is
the question [T-0115](T-0115-moving-elements-within-container-array-kills.md) left, which
this takes over. A block freed and handed out again at the same address would read as
unmoved, so the check wants a sequence number per allocation, which the memory tracker
(`memory/tracker.dz`) does not keep yet.
