---
id: D-0015
title: May `?` wrap a pointer or reference?
status: decided
---

**No.** `?` applies to value types only. `^T` is already the optional pointer; `(&T)?` would need lifetimes to be a library type and buys nothing as a builtin (§5.6)
