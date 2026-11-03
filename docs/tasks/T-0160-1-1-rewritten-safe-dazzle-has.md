---
id: T-0160
title: §1.1 rewritten: safe Dazzle has no undefined behaviour on one thread
milestone: none
status: open
kind: design
area: language
---

Point 8 of [D-0094](../decisions/D-0094-safe-code-has-no-undefined-behaviour.md), decided
as proposed on 2026-11-03 (r107).

**Where the code is**

| Piece | Where |
|---|---|
| The non-goal to rewrite | §1.1, "Memory safety as a guarantee" (`docs/spec/01-design-principles.md`) |
| The one-page summary that repeats it | §0.1 (`docs/spec/00-how-to-read-this.md`) |

**What it is.** §1.1 lists memory safety as a non-goal. D-0094 makes it a promise: a
program with no `unsafe` has no undefined behaviour on one thread. No read or write of freed
memory, no payload used under the wrong variant, nothing C++ leaves undefined. Aliasing stays
as §6.7 has it.

**Why it matters.** It is what the rest of D-0094 adds up to, and what a reader of the spec
needs to be told: where the guarantee ends is where to look for a bug.

**Why it is this way.** §1.1 was written before ties (D-0091, D-0093) took Dazzle most of
the way there, so it still says less than the language now does.

**What it would take.** Rewrite §1.1's first bullet, and §0.1 to match, once the holes it
promises are closed are closed: T-0126 and T-0127 (done), T-0128, T-0129, T-0135 and
T-0136. Name the exceptions: `unsafe` blocks, `std`'s trusted bodies (checked in debug
builds, T-0116), and threads (T-0011, v2). Done before then, the spec would promise what the
compiler does not keep. A §21 row says the promise is now in force.
