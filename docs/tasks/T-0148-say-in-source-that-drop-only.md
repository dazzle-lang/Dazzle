---
id: T-0148
title: Say in the source that a drop only frees, checked by the compiler
milestone: none
status: open
kind: design
area: ownership
---

Raised by the user on 2026-11-02, after [T-0118](T-0118-skip-drops-that-only-free-region.md);
for later.

**What it is.** A way to write on a type that its drop does nothing but free memory, so that
its drop may be skipped when a region frees what it owns. The compiler proves it either way
(T-0118); the marker would make the property visible where the type is declared.

**Why it matters.** Today only the analysis knows which drops only free. A reader, or an LLM
reading the code, cannot tell from the source whether a struct's drop will run at scope exit
or be left to its region; only the placement hint says so, and only where the type is used.
Written on the type, the property is part of its contract, and the compiler can hold the
type to it.

**Why it is this way.** r90 turned down a marker the compiler *trusts*, which would be wrong
the day the drop gains an effect. This one is *checked*: the compiler proves the drop only
frees, as T-0118 does, and a type marked so whose drop does more (closes a handle, changes a
count, calls C) is an error at the type, not a leak or a skipped effect somewhere else. The
marker adds no power, only a promise the compiler enforces.

**Open questions, for the user.**

- The spelling: an attribute on the type or on its `impl Drop` (`#[frees_only]`), or a word
  in the `impl Drop` line.
- Whether an unmarked type whose drop only frees still has it skipped (as now), or only a
  marked one does, which makes every skip visible in the source at the cost of writing it.
- Whether a generic type is marked once, the promise holding when its parameters' drops only
  free (`List<T>`), as the proof already works per instance.

**What it would take.** A decision (D-NNNN) and a §21 row, since it is new syntax; then the
check reusing `dz_flow/src/release.rs`'s `Drops::only_frees`, a diagnostic for a marked type
whose drop does more, and the marker on std's containers.
