---
id: T-0204
title: "`dz fix` adds a home to an owned result's `in` clause already written"
milestone: none
status: open
kind: feature
area: tooling
---

Left from [T-0200](T-0200-homes-built-sends-clauses-home-tie.md) stage 2, and split out of
[T-0202](T-0202-homes-through-traits-function-types-sends.md), which closed the safety half.
[D-0097](../decisions/D-0097-homes-ownership-that-travels-with-its.md)'s homes.

**Where the code is**

| Piece | Where |
|---|---|
| A body's sends and owned returns against its clauses, and the fix that writes one | `report_homes` in `dz_flow/src/ties/report.rs` (DZ0421, DZ0422) |
| The result's type and its `in` as written, so a fix knows where to write | `FnSig::ret_span`, `FnSig::ret_home` in `dz_check/src/lib.rs` |
| The fixes `dz fix` applies | `dz fix` in `crates/dz`; its tests in `crates/dz/tests/fix.rs` |

**What it is.** DZ0421's fix writes a missing `in x@home` after an owned result, but only
where no `in` is written yet. Where the result already says `in a@home` and the body also
returns what is in `b`'s home, the diagnostic is right and names the clause the body needs,
but `dz fix` does not rewrite `in a@home` as `in (a@home, b@home)`.

**Why it matters.** A convenience, not a hole: the program is refused until the clause is
right. But `dz fix` is how a clause is meant to be written (spec §6.7), and an LLM editing
Dazzle leans on it.

**What it would take.** `report_homes`' fix replaces the written `in` list with the one the
body needs, parenthesised when there is more than one path, as `dump::in_str` writes it; a
case in `crates/dz/tests/fix.rs`.
