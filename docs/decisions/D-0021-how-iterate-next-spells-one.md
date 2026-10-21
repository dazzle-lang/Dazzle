---
id: D-0021
title: How `Iterate::next` spells one signature for both item kinds
status: decided
---

**The question is void — there is no `next`.** `Iterate` is `valid` / `get` / `advance`, so termination is not encoded in the item and `Item` is never wrapped (§8.5). This **reverses the decided half of the old 18.21** ("a pointer iterator returns `^T`, a value iterator returns `Item?`"): that split existed only to dodge `(^T)?`, and with the item unwrapped it buys nothing. `Item` may now be `&T`, which `Item?` could never express, so §10.4's `for e in &entities` yields a `&Entity` as it always claimed
