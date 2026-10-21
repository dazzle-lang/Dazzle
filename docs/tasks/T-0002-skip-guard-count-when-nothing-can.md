---
id: T-0002
title: Skip the guard's count when nothing can drop the owner
milestone: M12
status: open
kind: feature
area: ownership
---

**Skip the guard's count when nothing can drop the owner.** An `if let` body with no
call the compiler cannot see into and no assignment to the handle cannot free the value,
so the count up and down could be left out. A compiler detail, not in the spec.
