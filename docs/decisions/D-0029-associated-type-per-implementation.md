---
id: D-0029
title: An associated type per implementation
status: decided
---

**Each implementation chooses its own.** The associated type belongs to the `impl`, not the type, so `impl Mul<Vec4> for Mat4` answers `Output = Vec4` and `impl Mul<f32> for Mat4` answers `Output = Mat4`. Falls out of 18.31; `Mul<Rhs, Output>` with a defaulted second parameter was the alternative, and would have needed a rule forbidding two outputs for one right-hand side (§8.5, §11.2)
