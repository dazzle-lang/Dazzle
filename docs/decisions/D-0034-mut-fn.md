---
id: D-0034
title: `mut fn`
status: decided
---

**Removed — the receiver says it.** A method mutates when it takes `&mut self` and reads when it takes `&self`; `fn resize(&mut self, n: u32)` is the one spelling. `mut fn` stated the same fact twice, and the two could disagree: `fn f(&mut self)` was accepted, and the compiler went on to treat the method as mutating in one place and as read-only in another. The parser now rejects `mut fn` with a help saying what to write (§7.3)
