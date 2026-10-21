---
id: D-0056
title: Variadic parameters
status: decided
---

**`name: ..T`, last, seen as a read-only `[]T`; `f(..xs)` spreads a slice as the only variadic argument; C varargs stay unimported.** The caller builds a temporary array, so there is no allocation and nothing is instantiated per call. Arguments of different types go through a trait reference (`..&Format`), which is what a library `println` needs. Chosen over same-type-only, which leaves `println` a builtin forever, and over compile-time packs (C++ `Ts...`), which cannot be walked without macros (M10). `..` rather than `...`: it already means "a run of" and needs no new token. A spread is explicit, so a `[]T` argument is never silently one element or many (§7.2). **"Last" is revised by 18.72**: there is at most one, and the parameters after it are passed by name
