---
id: D-0053
title: `T` where a `T?` is wanted
status: decided
---

**Converts, as at `return`.** A `T` becomes a `T?` wherever a `T?` is expected — a `let` with that type, an argument, an assignment — and where `if` or `switch` arms are joined and some arm is already a `T?` or `none`. Never the other way: a `T?` to a `T` still takes `or`. Chosen over `return`-only, which made state-machine `switch`es write every arm out as an optional (§9.1)
