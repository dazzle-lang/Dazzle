---
id: D-0051
title: How `for` gets an iterator from a container
status: decided
---

**Two prelude traits, `Iterable` and `IterableMut`**, each naming its iterator as an associated type: `fn iter(&self) -> Iter` and `fn iter_mut(&mut self) -> IterMut`. `for` over a value that is not itself `Iterate` calls `iter()`, or `iter_mut()` when a binding is `&mut`. Chosen over finding `iter()` by name, which states no contract and cannot be a bound: `fn total<C: Iterable>(c: &C)` must be writable (§8.5, §10.4)
