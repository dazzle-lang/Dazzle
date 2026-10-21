---
id: T-0059
title: Diagnostics for habits from Rust and C++
milestone: none
status: open
kind: feature
area: llm-tooling
---

**Diagnostics for habits from Rust and C++.** A model will keep writing
`Result<T, E>`, `Option<T>`, `x?`, `try`/`catch`, `use std::…`, `#include`, `nullptr`,
`auto`, `impl Foo { … }` without a trait, `Foo::new()`, `p->x`. The parser already
recovers from these; it should name the habit and give the Dazzle spelling ("this is
Rust's `?`; in Dazzle write `f() or return`", §9.5), with a fix where it is mechanical.
One diagnostic per position (invariant 6). Each one gets a `tests/errors` file.
