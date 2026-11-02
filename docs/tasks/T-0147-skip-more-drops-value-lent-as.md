---
id: T-0147
title: Skip more drops: a value lent as &mut, built by a trait method or format!, or in a generic body
milestone: none
status: open
kind: feature
area: ownership
---

Left out of [T-0118](T-0118-skip-drops-that-only-free-region.md), which skips a drop only where
it can prove the region frees everything the value owns, and answers "no" when unsure.

**What it is.** Four cases where a drop that only frees still runs, although the value is in a
region that frees it:

| The value | Why its drop still runs | What would let it skip |
|---|---|---|
| lent as `&mut` to a function, `fill(&mut xs)` | the callee could replace it with a value built elsewhere | follow the callee: whether it ever assigns its parameter whole, or hands it on as `&mut` |
| built by a trait method or a function value, `c.upto(n)` | which body builds it is not known at the call | every implementation of the trait method builds whole ([`whole_functions`]) |
| `format!`'s text, `let line = format!(…)` | the text is built inside the macro's lambda, not by a call the rule follows | treat the macro's value as the site its text is |
| in a generic body, `let tmp = List<T>()` | the type has a parameter, and one body is decided for every instance | decide per instance in codegen, which knows `T` |

A container of values that own memory, `List<String>`, is not on the list: its elements are
built on the heap until [T-0145](T-0145-value-stored-into-container-built-where.md) builds a
stored value where its container lives.

**Why it matters.** Each case keeps work at scope exit that the region's release already
does. Most of it is a call into the region's `free`, which gives memory back only when it was
the last allocation, so the cost is small. It grows with a container of owning elements,
which waits for T-0145 anyway.

**Where the code is.**

| Piece | Where |
|---|---|
| The rules: `skipped_drops`, `built_in_region`, `replaced`, `whole_functions`, `returns_whole` | `dz_flow/src/release.rs` |
| The test that pins what is kept today | `a_drop_is_skipped_only_when_its_region_frees_everything` in `dz_flow/tests/placement.rs` |

**What it would take.** Compiler work only (`docs/log/`). Each row is independent.
