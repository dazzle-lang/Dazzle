---
id: D-0064
title: `Rc<Derived>` where an `Rc<Base>` is wanted
status: decided
---

**Converts implicitly, as `&Derived` → `&Base` does (18.32)** and as C++'s `shared_ptr<Derived>` does: the same for `Weak` and `Box`. The count is shared, not copied. A handle to a trait object stays deferred, having no C++ counterpart to follow (§14.5)

**Amended by D-0104 (2026-11-14):** a `Box` is one word and converts only to a base with a `virtual` method (`DZ0588`), as a `unique_ptr` deletes soundly only through a base with a virtual destructor. `Rc` and `Weak` still convert to any base.
