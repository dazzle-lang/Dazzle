---
id: T-0044
title: `hash_combine` relies on `u64` arithmetic wrapping
milestone: M12
status: open
kind: deferral
area: stdlib
where: `dz_project/src/std/core/hash.dz`
---

**`hash_combine` relies on `u64` arithmetic wrapping**, which C++ guarantees and the spec does not yet say (§5 leaves overflow of plain arithmetic unstated, and signed overflow is not checked in debug as §5.1 proposes). `wrapping_add` exists now, and since r50 is in the same module. Deciding what plain `+` does on overflow settles both
