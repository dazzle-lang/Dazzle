---
id: D-0007
title: Keep the `!T` sigil?
status: decided
---

**No — `!` moves to the signature.** `T?` is a postfix type suffix; fallibility is marked `!->` on the arrow and is never a type. `Maybe<T>` / `Result<T,E>` stay rejected: the value in a built-in is compiler-known layout and no instantiation in every signature, and that survives the notation change (§9)
