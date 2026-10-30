---
id: D-0003
title: May a `&T` be stored in a struct field?
status: decided
---

**No** — banned outright, with no escape hatch. References live only on the stack; context structs hold `^T` (§5.2)

**Refined by [D-0093](D-0093-views-are-borrowed-no-bare-str.md)** (r89, r90): a `view struct`
may hold references. It is itself placed only where a reference may be, so references still
live only on the stack. Not yet implemented ([T-0111](../tasks/T-0111-view-structs-struct-that-holds-view.md)).
