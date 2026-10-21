---
id: D-0049
title: Evaluation order, and pointers as numbers
status: decided
---

**Arguments left to right, receiver first**, as they are written — C++'s unspecified order is not inherited, so a program means one thing on every compiler. Since r44 the same for operands, indexing, slice and range bounds, and literal elements, with struct fields in written order; **assignment evaluates its value before its place**, as C++17 and Rust do. **`^T as usize` is allowed, `usize as ^T` only inside `unsafe`**: reading an address is harmless and allocators need it to align and to hash; making a pointer from a number is how memory gets corrupted (§7.2, §10.6)
