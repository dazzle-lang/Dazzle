---
id: T-0050
title: Associated-type projection
milestone: M12
status: open
kind: deferral
area: checker
where: `dz_check/src/check.rs`, `iterate_item`
---

**Associated-type projection** (`T::Item`, `T::Output`). An associated type resolves through an implementer but cannot be named off a bounded parameter, so `for` over a generic `T: Iterate` is refused by the checker (`DZ0801`), and generic arithmetic cannot name its result type

**Why a game needs it** (from the readiness review, 2026-09-14):

**Associated-type projection** — `T::Item`, `T::Output` off a bound. Unblocks `for`
over a generic `C: Iterable`, generic arithmetic, and real iterator adapters.
