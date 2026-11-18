---
id: T-0259
title: Closures (D-0112, spec §7.8)
milestone: M19
status: open
kind: feature
area: language
where: none yet; the design is D-0112, the rule spec §7.8 (r144); `crates/dz_project/src/std/collections/list.dz` (`sort_by`, which takes a plain `fn` today)
---

Asked by the user on 2026-11-18 ("we need closures") and decided the same day: `fn(a, b) {
… }`, captures by reference worked out from use, a view never stored, typed `view fn(…)`.
Spec §7.8; the decision and its reasons are [D-0112](../decisions/D-0112-closures.md).

It was asked for so a parallel loop's body could be written inline; that became a `for` over
a parallel iterator instead (spec §22.3), so threads no longer wait on it. **First uses:**
`List::sort_by` taking a `view fn` (a plain function still converts), then iterator `map`
and `filter`. Owning closures, which may be stored or given to `spawn!`, are not in it.
