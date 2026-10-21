---
id: D-0013
title: Is `machine` native syntax or a macro?
status: decided
---

**A macro, written in user code** — it belongs in your engine, not the stdlib. It may graduate to the stdlib later if it proves general. It stays the benchmark the macro system must be good enough for (§12.1)
