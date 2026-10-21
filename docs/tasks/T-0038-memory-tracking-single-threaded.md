---
id: T-0038
title: Memory tracking is single-threaded
milestone: M12
status: open
kind: deferral
area: stdlib
where: `dz_project/src/std/memory/tracker.dz`
---

**Memory tracking is single-threaded.** `TRACKER` is one static with no lock, so two threads allocating from `Global` under `--track-memory` race — concurrency is v2 (18.11)
