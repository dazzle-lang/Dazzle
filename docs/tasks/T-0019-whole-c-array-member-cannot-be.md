---
id: T-0019
title: A whole C array member cannot be copied or passed as a slice
milestone: M7b
status: open
kind: deferral
area: checker
where: `dz_check/src/check.rs`, `is_c_array_member`
---

A whole C array member is refused rather than copied (`DZ0707`), and cannot be passed as a `[]T`. Copying one needs a helper that fills a `dz::Array` element by element; the slice needs a `Slice{ptr, len}` built at the use site. Indexing, `for`, `.len` and `.ptr` all work
