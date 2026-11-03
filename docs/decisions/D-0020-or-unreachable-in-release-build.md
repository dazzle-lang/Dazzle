---
id: D-0020
title: `or unreachable` in a release build
status: decided
---

**The compiler assumes the success path** — `__builtin_unreachable`, not merely a skipped check. Debug still panics with the location. Chosen for speed: `or unreachable` is already the loud spelling of "this cannot fail", and §1.1 does not promise safety (§9.4)

**Revised by D-0094 point 4 (r108)**: `or unreachable` panics with the location in every build. The assumption is kept only inside `unsafe`, a block or an `unsafe fn`, where an optimised build still lowers it to `__builtin_unreachable`: a release build that assumed a failed search succeeded ran the loop past the end of its list, undefined behaviour reached from safe code (T-0128).
