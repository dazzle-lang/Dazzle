---
id: T-0115
title: Moving elements within a container's array kills nothing
milestone: none
status: dropped
dropped: r90, 2026-10-30
kind: deferral
area: ownership
---

**Dropped, 2026-10-30 (r90), by the user's choice: folded into
[T-0116](T-0116-debug-build-check-std-trusted-kills.md).** T-0116's debug check tests these
bodies, `[*]` exactly. Teaching the analysis instead would special-case a `move_out` paired
with a `construct`, and still leave the bodies trusted: `memmove` shifts and `memcpy` appends
are pointer arithmetic, and growing (`move_to`) moves every element into a fresh buffer that
becomes `m_data` only afterwards, which the analysis cannot follow.

Left over from T-0107's step 2d ([T-0107](T-0107-design-ties-kills-clauses-compiler-placed.md)).

**What it is.** `List`'s bodies over its `m_data` pointer are trusted rather than checked
against their clauses (§6.7: "`std`'s own clauses are written by hand over `unsafe` code, and
are trusted"). The ones that only move elements within the array — `swap`, `reverse`,
`sort_by`, the shifts in `insert` and `remove`, growing, `append` — read to the analysis as
killing `self[*]`, since it cannot tell a move from one slot of `[*]` to another from a move
out of the container. Their clauses rightly say they do not (§6.7's table: `sort_by` kills
`self@items` only).

**Why it matters.** A trusted body can be wrong without anything noticing; every body the
analysis can hold to its clause is one fewer trusted. It matters less once
[T-0116](T-0116-debug-build-check-std-trusted-kills.md) tests trusted clauses in debug builds.

**Why it is this way.** The analysis sees `move_out` and `construct` on element places, and
`[*]` names every element at once, so a move between two of them looks like one leaving.

**What it would take.** Compiler work only (`docs/log/`). Either the analysis learns that a
`move_out` from one slot of `[*]` followed by a `construct` into another slot of the same
`[*]` kills nothing, or these bodies stay trusted until T-0116.

| Piece | Where |
|---|---|
| The trusted-body marker, pointer indexing to `[*]` | `index_origins` in `dz_flow/src/ties/body.rs` |
| `List`'s bodies | `dz_project/src/std/collections/list.dz` |
