---
id: T-0003
title: A handle's `or` with a value fallback
milestone: M12
status: open
kind: feature
area: ownership
---

**A handle's `or` with a value fallback.** `rc or 7` is refused, as `ptr or 7` is: the
success side is a `&T`. `if let v = rc { v } else { 7 }` works. Worth deciding for both
pointers and handles together.
