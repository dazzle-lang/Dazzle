---
id: D-0064
title: `Rc<Derived>` where an `Rc<Base>` is wanted
status: decided
---

**Converts implicitly, as `&Derived` → `&Base` does (18.32)** and as C++'s `shared_ptr<Derived>` does: the same for `Weak` and `Box`. The count is shared, not copied. A handle to a trait object stays deferred, having no C++ counterpart to follow (§14.5)
