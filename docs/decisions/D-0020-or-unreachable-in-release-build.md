---
id: D-0020
title: `or unreachable` in a release build
status: decided
---

**The compiler assumes the success path** — `__builtin_unreachable`, not merely a skipped check. Debug still panics with the location. Chosen for speed: `or unreachable` is already the loud spelling of "this cannot fail", and §1.1 does not promise safety (§9.4)
