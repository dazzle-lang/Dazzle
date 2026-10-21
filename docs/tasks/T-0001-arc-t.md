---
id: T-0001
title: `Arc<T>`
milestone: v2
status: open
kind: feature
area: ownership
---

**`Arc<T>`** — atomically counted `Rc`. Waits on atomics, which wait on concurrency
(18.11, v2); listed so the name is reserved and `Rc`'s API is shaped to match.
