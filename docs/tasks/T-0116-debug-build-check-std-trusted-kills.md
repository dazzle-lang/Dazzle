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

**What it would take.** Compiler and runtime work (`docs/log/`): a debug-only field or side
table per container allocation, views carrying the generation in debug builds (which changes
their C++ layout in debug only), checks at each use, and tests that hand-break a clause and
expect the trap. Design to settle first: what a view carries, and how release builds stay
free of it.
