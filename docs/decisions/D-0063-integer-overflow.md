---
id: D-0063
title: Integer overflow
status: decided
---

**Wraps, in every build; never panics.** Replaces §5.1's proposal of a panic in debug. What every mainstream language but Rust and Swift does, and what hashing code relies on; signed overflow included, which C++ leaves undefined and Dazzle does not. The `Integer` methods say anything else (`checked_add`, `saturating_sub`) (§5.1)

**Extended by D-0094 point 5 (r109)**: the rest of integer arithmetic is defined the same way. `MIN / -1` wraps to `MIN`, `MIN % -1` is 0, a shift count is taken modulo the width, and a negative value shifted left wraps. A division by zero, which has no answer to wrap to, panics (T-0129).
