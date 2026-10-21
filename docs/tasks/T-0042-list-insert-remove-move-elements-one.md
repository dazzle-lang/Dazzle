---
id: T-0042
title: `List::insert` and `remove` move elements one at a time
milestone: M12
status: open
kind: deferral
area: stdlib
where: `dz_project/src/std/collections/list.dz`
---

`List::insert` and `remove` move elements one at a time, even for a `T` whose bytes could be moved in one `memmove`; there is no `memmove` builtin yet
