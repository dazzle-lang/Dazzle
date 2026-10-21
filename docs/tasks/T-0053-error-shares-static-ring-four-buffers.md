---
id: T-0053
title: `error(…)` shares a static ring of four buffers
milestone: M12
status: open
kind: deferral
area: codegen
where: `dz_cpp/src/runtime.rs`, `message_buffer`
---

**What it does.** `error(…)` (§9.1) builds a failure's message. §9.2 demands two things of
it at once: **no allocation** — a failure must be reportable when the allocators are the
thing that failed — and **a message that outlives the call**, since an `Error` is returned
and read by the caller. Static storage is the only thing that satisfies both, so the shim
writes into a **ring of four 256-byte buffers** (`message_buffer`).

So the **fifth live message overwrites the first**:

```dz
let a = open("1") or return error("could not open {}", "1")
// ... four more failures later, `a`'s message has been overwritten
```

**Kept by the user's decision** (2026-09-13, `docs/log/m12.md`). This is not an oversight:
given §9.2's two requirements there is no allocation-free answer that is also unbounded, and
four concurrent live messages is a generous bound for a language with no threads. The row
exists to record the limit, not to ask for it to be lifted.

**What has already improved.** `format` no longer shares the ring — it is `String::format`
since r62 and `format!` since r64 — so the only thing competing for the four slots is
`error` itself. Before that, formatting and error messages could evict each other, which was
much easier to hit.

**When it would bite.** Holding several errors at once: collecting failures from a batch of
asset loads to report them together, or an error carried up through several layers each
adding context. Both are plausible in a loader, and the symptom is a *wrong message* rather
than a crash — the worst kind, since it misdirects whoever is debugging.

**What could be done, if it ever matters.**

- **Make the bound visible.** Nothing tells the user there are four slots, and nothing fires
  when the fifth is taken. A debug-build assertion, or a warning when a slot is reused while
  its message is still referenced, would turn a silent wrong answer into a reported one.
  This is the cheap half and probably the right half.
- **Raise the count**, which costs static memory and moves the cliff rather than removing it.
- **Let an `Error` carry an owned `String`** where the failure is not an allocation failure —
  which means two kinds of error, and §9.1's whole point is that there is one.

The first is worth doing; the other two change §9.2's bargain and would need the user to
revisit the 2026-09-13 decision.
