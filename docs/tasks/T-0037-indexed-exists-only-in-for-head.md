---
id: T-0037
title: `indexed()` exists only in a `for` head
milestone: M12
status: open
kind: deferral
area: checker
where: `dz_check/src/check.rs`, `for_stmt`
---

**`indexed()` exists only in a `for` head.** It is recognised there by the checker, not declared: a real adapter is a generic struct implementing `Iterate` over another, whose `Item` needs associated-type projection (the row below). `move(xs).indexed()` is refused for the same reason

**Why a game needs it** (from the readiness review, 2026-09-14):

**Iterator adapters** — `indexed()` as a real adapter rather than a `for`-head
special case, then `filter`, `map`, `rev`, `zip`.
