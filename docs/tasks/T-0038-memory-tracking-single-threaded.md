---
id: T-0038
title: Memory tracking is single-threaded
milestone: M12
status: open
kind: deferral
area: stdlib
where: `dz_project/src/std/memory/tracker.dz`, `TRACKER`
---

**What happens now.** `--track-memory` records every allocation from `Allocators::Global`
with its source location, so a leak report can name the line that allocated. The records
live in `TRACKER`, **one static with no lock**. Two threads allocating at once race on it:
the count, the table and the live-allocation list are all read-modify-write with no
synchronisation, so the outcome is a wrong report at best and a corrupted table at worst.

**Why it is accepted.** Dazzle has no threads. Concurrency — threads, a mutex, atomics, a
job system — is **v2** by D-0011 ([T-0011](T-0011-threads-mutex-atomics-job-system.md)), so
there is no way to write the program that would trigger this. The single-static design is
the right one for a single-threaded language: no lock means tracking costs an increment and
a table write, which is what makes it cheap enough to leave on during development.

**Why it is listed anyway.** Because it is a trap for whoever adds threads: the tracker is
not obviously shared state when you are looking at `Thread::spawn`, and a corrupted leak
report is the kind of failure that gets blamed on the new feature for a week. Writing it
down is most of the value of this row.

**What it would take.** It is listed as a prerequisite on
[T-0011](T-0011-threads-mutex-atomics-job-system.md) and should be done as part of it, not
before: once atomics exist, the counters become atomic and the table takes a mutex, and the
only real question is whether the mutex is acceptable on the allocation path or whether the
tracker should become per-thread with a merge at report time. The second is the usual answer
and is the one to reach for, but it is not worth designing until there is a thread to put it
on.
