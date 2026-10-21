---
id: D-0039
title: How is a `cstr` made?
status: decided
---

**A string literal is one wherever a `cstr` is wanted**, typed by its context as an untyped number is, and emitted as the C literal itself: it is zero-terminated static storage already, so nothing is allocated. `"x" as cstr` is the explicit spelling; `c as str` measures a C string. A runtime `str` has no terminating zero, so making a C string from one needs an owned copy, freed by its drop — which arrives with `String` (M12) and is spelled then. This replaces §5.3's `[PROPOSED]` "conversion to `cstr` is explicit and may allocate", which contradicted §15.1's own `InitWindow(…, "Dazzle")` (§5.3)
