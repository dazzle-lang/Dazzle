---
id: D-0030
title: `Error.location` is a `^T`
status: decided
---

**It is a `&SourceLocation`**, the one reference the language stores in a struct: the location lives in static storage for the whole program, so the reason §18.3 bans stored references does not apply. `e.location.line` reads it directly, and `Error` stays 32 bytes. User structs still cannot store a reference (§9.2)
