---
id: D-0035
title: Reading a `&T` as a `T` — the open half of 18.25
status: decided
---

**Implicit, for a copyable `T`.** A reference is accepted where its referent's type is wanted, and the value is copied: `for m in list.iter() { if m.name == name { return m } }` returns a `Movie`. Reading cannot fail, so by the rule that makes `^T` → `&T` loud and `&mut T` → `^T` quiet, it is not spelled. A move-only `T` is still refused, as a move out of a borrow; a generic body needs `T: Copy`. An explicit `r^` was considered and rejected — it would put a sigil on the most ordinary read in the language — and `^` stays the pointer dereference (§5.2)
