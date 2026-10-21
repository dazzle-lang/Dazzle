---
id: D-0032
title: Struct inheritance
status: decided
---

**Single inheritance, written `struct Button : UiElement`.** The base comes first in memory, so `&Derived` → `&Base` and `^Derived` → `^Base` are implicit and free, while a by-value `Derived` where a `Base` is wanted is an error — C++'s slicing, which drops what the derived type adds. A base's fields and methods are the derived type's, unqualified inside its methods, and neither may be redeclared. A base with a constructor is constructed through it, `Button(…) : UiElement(r)`, and a type with a constructor is not built with a struct literal. Dispatch is static unless written: `virtual fn` opts one method in, `override fn` replaces one, both checked in both directions, and a body-less `virtual fn` makes the struct abstract — no value of it exists, only a `&` or a `^` to one. `Base::method(self)` calls the base's version without dispatch, and `drop` becomes virtual as soon as anything in the chain is. A fourth visibility, `protected`, opens a member to the types that inherit it (§4). Implementations are inherited and a more specific `impl` replaces the inherited one, statically. This reverses §7.3's old "there is no inheritance", which was never a decision of the language's author (§7.3, §4)
