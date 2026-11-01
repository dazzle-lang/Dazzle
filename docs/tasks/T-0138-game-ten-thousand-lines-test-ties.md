---
id: T-0138
title: A game of ten thousand lines, to test ties and placement at scale
milestone: none
status: open
kind: feature
area: language
---

**What it is.** The largest Dazzle program is about 1,000 lines (`examples/sdl3`), and all
the examples together are about 2,400. Ties and placement were designed against scenarios
(T-0107) and checked on that corpus. Neither has met a program the size of a real game,
which is where their costs show:

- how often a `kills` clause changes when a body does ([T-0137](T-0137-kills-clause-must-be-exact-so.md));
- how often [T-0123](T-0123-call-may-reach-memory-it-kills.md)'s rule refuses code that is fine;
- how much `unsafe` real code needs, and where;
- what regions do to speed and peak memory over a whole frame loop
  ([T-0117](T-0117-memory-placed-by-compiler-regions-result.md),
  [T-0139](T-0139-frame-regions-one-region-per-pass.md));
- how long a build takes ([T-0012](T-0012-measure-compile-time-at-scale.md)).

**Why it matters.** It is the difference between a design that works on examples and a
language people can use. Every open question about the cost of ties is answered by this,
and no other way. It is also the "real program with a container of owning values cleared
each frame" that [T-0120](T-0120-owner-arenas-for-containers-cleared-all.md) waits for.

**What it would take.** A game of 10,000 lines or more, written the way Dazzle means to be
written: entities in a `SlotMap`, assets in `Rc`, per-frame scratch data, and a few C
libraries through `import cpp`. Whatever hurts is recorded as a task. Best done after
D-0094's call-site rule (T-0123), so it meets the rules as they will be.
