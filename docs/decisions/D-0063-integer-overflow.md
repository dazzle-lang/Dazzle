---
id: D-0063
title: Integer overflow
status: decided
---

**Wraps, in every build; never panics.** Replaces §5.1's proposal of a panic in debug. What every mainstream language but Rust and Swift does, and what hashing code relies on; signed overflow included, which C++ leaves undefined and Dazzle does not. The `Integer` methods say anything else (`checked_add`, `saturating_sub`) (§5.1)
