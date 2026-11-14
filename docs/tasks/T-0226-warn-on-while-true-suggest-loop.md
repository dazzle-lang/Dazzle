---
id: T-0226
title: Warn on while true and suggest loop, which dz fix rewrites
milestone: none
status: open
kind: feature
area: tooling
where: `crates/dz_check/src/check.rs`: `StmtKind::While`; `crates/dz_diag/src/codes.rs`; `dz fix`'s edits
---

**For 0.3, with the work on `dz fix`**, the user's plan of 2026-11-14 (D-0102).

`loop { … }` is the loop left only by `break` or `return` (r131). A `while true { … }`
does the same work but is not seen as never ending, so a function ending in one needs a
`return` after it that cannot run. The compiler should warn on a `while` whose condition is
the literal `true`, saying to write `loop`, with an edit `dz fix` applies and the editor
offers as a quick fix: `while true {` becomes `loop {`, a label kept (`outer: while true` →
`outer: loop`). The warning needs a code of its own in the warnings range of
`crates/dz_diag/src/codes.rs`, and `dz explain` text.

Nothing in the repository writes `while true` any more but the spec's own examples of it
(§10.2), which should become `loop` at the same time.
