---
id: D-0041
title: What a C `union` becomes
status: decided
---

**A type whose members overlap**, read and written as C does it — not a Dazzle `union`, which is tagged (§7.5) and would promise a discriminant the C type does not have, with an exhaustive `switch` that means nothing. So `RGFW_event` comes in with all its members, `e.type` reads the one C would, and reading a member nobody wrote is C's own bargain rather than something Dazzle can check. A literal may set **one** member, since they are the same bytes (`DZ0705`); the type is never defined by Dazzle, so its layout stays the header's (§15.2)
