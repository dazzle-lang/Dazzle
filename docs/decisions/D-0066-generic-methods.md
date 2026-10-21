---
id: D-0066
title: Generic methods
status: decided
---

**A method may declare type parameters of its own, solved per call like a generic function's; written after `.name` as they are after a path, `xs.map<f32>(f)`, with no turbofish.** The `<…>` after a field name is read as type arguments only when `(` follows the `>`, so `n.len < max` is unaffected — the same non-associativity argument as §3.3. Not allowed on `virtual`/`override` methods or in a trait (`DZ0557`), where one function would have to serve every argument; a generic trait says the same thing and works through a `^Trait`. Each instance is emitted as a free function taking the receiver, not a C++ member template, so §8.6 still holds (§8.1)
