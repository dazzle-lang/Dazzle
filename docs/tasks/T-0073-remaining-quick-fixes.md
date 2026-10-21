---
id: T-0073
title: The remaining quick fixes
milestone: M9
status: open
kind: feature
area: editor
---

**Quick fixes** — the mechanism and the first two (fill a `switch`'s missing arms,
implement a missing trait method) are **done**; see `docs/log/m9.md`. The rest are the ones the compiler already works the answer out for, and
each is now a few lines at the diagnostic's own site — attach a `Fix`, and it appears in
the editor: insert `move()` where a value was implicitly copied, add the missing
`override`, add `: Base(…)` to a constructor that needs one, add a missing `impl Drop`.
