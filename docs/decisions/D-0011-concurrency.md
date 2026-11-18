---
id: D-0011
title: Concurrency
status: decided
---

**Deferred to v2.** Threads, atomics, and a job system go in the stdlib once the core language is stable; nothing here forecloses them

The design for when that time comes, still open: [D-0096](D-0096-threads-safe-multithreading-built-on-ties.md) (2026-11-10).

**Scheduled, 2026-11-18:** the user put threads in 0.3, as roadmap milestone M19 (§19). The
first release this deferral waited for was 0.1; the work is [T-0245](../tasks/T-0245-try-break-d-0096-before-deciding.md)
(deciding D-0096), then [T-0011](../tasks/T-0011-threads-mutex-atomics-job-system.md).
