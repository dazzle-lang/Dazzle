---
id: D-0016
title: Implicit `&T` → `^T` widening, and what a raw pointer may do
status: decided
---

**Forming a pointer is safe; using one is `unsafe`.** `&mut T` → `^T` stays implicit; `&T` → `^T` is forbidden, since a `^T` may be written through and a `&T` promises it will not be. Reading or writing through a `^T` (`p^`, `.`, `[i]`), a pointer cast, `construct`/`destroy`/`memcpy`/`free`, and calling an `unsafe fn` are allowed only inside `unsafe { }` or an `unsafe fn`. The `or` conversion stays safe and yields a `&mut T`. `unsafe` is a marker, not a proof: it promises that raw-memory operations are findable, not that safe code is free of undefined behaviour. Array and slice indexing is therefore safe and bounds-checked in every build, with `xs.ptr` as the unchecked spelling (§5.2, §5.3). An explicit `as ^T` was rejected because it makes forming the pointer loud and leaves using it silent — the opposite of where the danger is. **Its first half is revised by 18.37**: a pointer now says whether it may be written through, so `&T` → `^T` is allowed — a `^T` is only read — and it is `&T` → `^mut T` that is refused
