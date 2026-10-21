---
id: D-0022
title: Is a slice writable through an immutable binding?
status: decided
---

**Neither — writability moves into the type.** `[]T` is a read-only view and `[]mut T` a writable one, standing to each other as `&T` and `&mut T` do. Following the binding was rejected because a slice is a view, so the binding says nothing about the elements (§5.3)
