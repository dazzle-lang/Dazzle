---
id: D-0025
title: Can a `&T` be read as a `T`?
status: decided
---

**Not yet — and the operator traits stop needing it.** `&T` reaches through `.` for members and is otherwise not a value: `fn f(b: &f32) -> f32 { return b }` is an error, and `^` derefs a `^T` only (§5.2). That made §11.2's `fn mul(&self, rhs: &Rhs)` unimplementable for `Mul<f32>`, so `Rhs` is taken **by value** instead, which is the better signature for the copyable math types these traits exist for. `Eq`/`Ord` keep `&Self`, because comparing a move-only type must not consume it. Whether `&T` should gain a read of its own is left open — nothing needs it now. **Settled by 18.35**: it reads as a `T` where one is wanted
