---
id: T-0040
title: `for` over a generic `C: Iterable`
milestone: M12
status: open
kind: deferral
area: checker
where: `dz_check/src/check.rs`, `container_iterator`
---

**`for` over a generic `C: Iterable`** is refused (`DZ0801`): the loop's element is the projection of `C::Iter`'s `Item`, the same associated-type projection a generic `T: Iterate` loop waits on. Concrete containers loop (r47)
