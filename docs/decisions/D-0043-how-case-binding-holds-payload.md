---
id: D-0043
title: How a `case` binding holds the payload
status: decided
---

**Exactly as a `for` binding (18.42).** `case Circle(c)` is a copy that cannot be changed, `&c` a read-only reference, `&mut c` a writable one that needs a `let mut`, `&mut` or `^mut` subject. One rule for both places; a binding that was always a reference would make `&mut c` the only mode worth writing. The binding had been writable always, which let a `switch` change a value declared immutable and failed in C++ behind a `&` (§10.3)
