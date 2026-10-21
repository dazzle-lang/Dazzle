---
id: D-0036
title: Using a `T?` when both outcomes continue
status: decided
---

**`if let name = value { … } else { … }`.** The binding holds what `or` would produce — the `T` of a `T?`, the `&mut T` of a `^T` — in the first block only. Chosen over narrowing `m` after `if m != none`, which needs flow-sensitive types and works only on locals, and over "`or` only", which leaves "both branches, then carry on" with no spelling. It does not take a fallible call, whose `else` would drop the `Error` unseen (§9.5); binding the error in `else` is left for later. `m == none` / `m != none` test presence; two optionals do not compare (§9.4). *How the binding holds the value is revised by 18.61:* `if let p = ptr` is now a copy, and the `&mut T` is `if let &mut p = ptr`
