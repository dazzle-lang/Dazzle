---
id: D-0009
title: Macro invocation syntax
status: decided
---

**One spelling, `#name`, in every position** (reversed in r63 by the user; it had been `#name …` for declarations and `name!(…)` for expressions). In declaration and statement position an invocation runs to the end of the line, in expression position it is one bracketed group; the intrinsics are the compiler's own macros, invoked the same way. `!` already meant "not" and "fallible" (`!->`), and `#` already meant "the compiler does something here" (§12.5, §12.8)
