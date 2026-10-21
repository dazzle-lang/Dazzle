---
id: D-0042
title: How a `for` binding holds its element
status: decided
---

**On the binding, as C++ does it: `e` a copy, `&e` a reference, `&mut e` a writable reference.** `e` cannot be changed, so a write meant for the element cannot silently land on a copy. A copy is a real copy, never a reference in disguise, since a `copy` or `drop` with side effects would otherwise run or not depending on the compiler. Replaces `for e in &xs` / `for e in &mut xs`, where the sequence expression said it. Several names always take a tuple element apart, with no parentheses — `for id, &btn, &mut state in query` — and a pass count comes from `xs.indexed()`, so `for i, e in xs` no longer means an index: two readings of two names was the conflict this settles. What an iterator's `Item` is decides which bindings its loops may use (§10.4)
