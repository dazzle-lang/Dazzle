---
id: D-0003
title: May a `&T` be stored in a struct field?
status: decided
---

**No** — banned outright, with no escape hatch. References live only on the stack; context structs hold `^T` (§5.2)
