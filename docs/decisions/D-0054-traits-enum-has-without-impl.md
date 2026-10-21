---
id: D-0054
title: Traits an enum has without an `impl`
status: decided
---

**`Eq`, `Hash` and `Ord`, as the integers they are.** Equality and order compare values — declaration order when values are implicit — and a hash is the value's. So an enum is a `HashMap` key and satisfies `T: Ord` with nothing written (§7.4)
