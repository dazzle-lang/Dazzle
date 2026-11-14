---
id: D-0006
title: Is `^Trait` one pointer or two?
status: decided
---

**Two — a fat pointer.** `{ data, vtable }`, vtable in static storage, implementing types unchanged. `Vec3 : Drawable` is still 12 bytes and still blittable (§7.6)

*Amended by D-0103 for one trait:* a `^mut Allocator` is one word, and each allocator keeps
its vtable as a hidden first field, because every container stores one.
