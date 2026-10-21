---
id: T-0001
title: `Arc<T>`
milestone: v2
status: open
kind: feature
area: ownership
---

**What it is.** An atomically counted `Rc` — the same handle (§14.5), with the share count
incremented and decremented atomically so that handles to one value may be held and dropped
by several threads.

**Why it is v2 and not M12.** It waits on **atomics**, which wait on **concurrency**, which
D-0011 places after the first release. There is no way to write a program that needs it:
Dazzle has no threads, so an `Arc` today would be an `Rc` with a more expensive counter and
nothing to justify it.

**Why it is listed anyway.** Two reasons, and they are the point of the row:

1. **The name is reserved.** `Arc` is what this will be called, so nothing else takes it, and
   a user reading `std` can see that the thread-safe variant is planned rather than missing.
2. **`Rc`'s API is shaped to match.** This is the part that costs something if it is
   forgotten. `Rc`, `Weak` and the `Nullable` trait were designed so that `Arc` is a drop-in:
   `Rc<T>` and `Weak<T>` share an `RcHeader` carrying the counts and the value's drop
   function (r59, handles-to-a-base), so an `Arc` is the same layout with atomic counters and
   the same `Nullable` implementation. Any change to `Rc`'s shape should be checked against
   that — if `Rc` grows a method or an invariant that cannot be made atomic, the drop-in
   stops being one and this becomes a redesign rather than a variant.

**What it would take, when the time comes.** Atomics; an `AtomicU32`-equivalent in the
counts rather than plain ones; a decision about whether `Arc<T>` requires `T` to be
thread-safe (Rust's `Send`/`Sync`, which Dazzle has no analogue of and may not want — §1.1
does not promise safety, so "a data race is the programmer's problem" is a consistent
position and worth deciding explicitly rather than by default); and a thread-safe memory
tracker ([T-0038](T-0038-memory-tracking-single-threaded.md)), since an `Arc`'s allocation
goes through the same `Global`.

All of it belongs to [T-0011](T-0011-threads-mutex-atomics-job-system.md) and should be done
as part of it, not before.
