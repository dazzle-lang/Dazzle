---
id: D-0061
title: How an `if let` binding holds the value
status: decided
---

**As a `for` or `case` binding does (18.42, 18.43): `v` a copy that cannot be changed, `&v` a read-only reference, `&mut v` a writable one** — for an optional, a pointer and a handle alike. `&mut` needs a writable pointer, reference or place (`DZ0507`); `v` needs a copyable value wherever the `if let` does not own it (`DZ0403`). This **reverses the pointer half of 18.36**, where `if let p = ptr` gave a `&mut T`: with handles added, one rule in four places was chosen over three rules and an exception, and `p` meaning a reference on a pointer but a copy on everything else would have been that exception. A `&T?` is read as its optional by `or` and `if let` both, as 18.35 reads a `&T` as a `T` (§9.4)
