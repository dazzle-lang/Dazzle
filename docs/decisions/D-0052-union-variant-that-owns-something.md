---
id: D-0052
title: A union variant that owns something
status: decided
---

**§7.5's proposal adopted: a union owns its active payload.** It is dropped with the union, dropped before another variant is assigned, and moved by moving the active payload. A union is copyable only when every variant is. This is also what lets a variant carry a struct with a constructor, `copy`, `drop`, base or `virtual` method (§7.5)
