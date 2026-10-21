---
id: D-0078
title: Methods on unions
status: decided
---

**A union may declare methods, as a struct does; `self` is the union.** No constructors, `copy` or `drop`, which its variants already define (18.52). Needed by `#machine`, whose `step` must belong to the machine it steps: as a free function two machines in one module would collide, with no overloading to tell them apart (§7.5)
