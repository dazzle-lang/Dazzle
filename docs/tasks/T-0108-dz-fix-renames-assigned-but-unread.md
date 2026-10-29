---
id: T-0108
title: dz fix renames an assigned but unread variable only where it is declared
milestone: M9
status: open
kind: bug
area: tooling
where: `dz_flow/src/lib.rs`, `unused`
---

**What it is.** `DZ0406` warns about a variable nothing reads, and carries a certain fix,
"rename it `_n`", which `dz fix` applies unattended. For a variable that is *assigned* but
never read, the fix renames only the declaration, so the program stops compiling:

```dz
fn tally(xs: []i32) {
    let mut n = 0        // DZ0406: `n` is assigned but never read
    for x in xs {
        n += 1           // after `dz fix`: DZ0301, `n` is not found
    }
}
```

Found on 2026-10-29 running `dz fix` over `tests/workspace/typed` for T-0107's clauses: line
899 of `typed.dz` broke this way.

**Why it matters.** `dz fix` is meant to be safe to run unattended, and an LLM or a script
runs it exactly so. A certain fix that breaks the program undoes that.

**What it would take.** In `unused` (`dz_flow/src/lib.rs`), either rename every assignment
too — the HIR has each `Assign` whose place is the local, with its span — or, for the
assigned case only, offer the rename as a suggestion (`with_suggestion`) rather than a fix.
Compiler work only (`docs/log/`).
