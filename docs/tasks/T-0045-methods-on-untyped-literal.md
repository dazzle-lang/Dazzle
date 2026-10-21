---
id: T-0045
title: Methods on an untyped literal
milestone: M12
status: open
kind: deferral
area: checker
where: `dz_check/src/check.rs`, method lookup on a literal
---

**What goes wrong.** A numeric literal is **untyped** until a context gives it a type — that
is what lets `let x: u8 = 3` and `let y: f64 = 3` both work with no suffix and no conversion.
But a method call *is* a context that needs the type already, and a literal on its own has
none, so there is nothing to look `Float`'s or `Int`'s methods up on:

```dz
let r = 2.0.sqrt()          // refused: no type yet to find `sqrt` on
let n = 255.count_ones()    // the same
```

Both workarounds are one token:

```dz
let r = (2.0 as f32).sqrt()
let x: f32 = 2.0
let r = x.sqrt()
```

**Why it matters.** It is a small, frequent annoyance rather than a blocker, and it is
annoying in a specific way: the error arrives at a spelling that looks obviously fine and
reads like the method does not exist, when in fact the receiver has no type. A game does
arithmetic on constants constantly — `0.5.min(t)`, `2.0.sqrt()` — so this comes up early and
to everybody.

**What it would take.** The natural fix is to apply the **literal's default type in a
receiver position**, where no context can supply one: `f64` for a float literal, which D-0001
already settles, and the integer default for an integer one. The rule would then be an
extension of an existing one rather than a new idea. The risk is that it makes a type
silently appear where the language's whole stance is that numeric types are explicit, and
that it is a trap when the default is not what the surrounding expression wanted —
`2.0.sqrt()` would be an `f64`, so assigning it to an `f32` is then a second error at a
different place, blaming the wrong line.

So the cheaper and more Dazzle-ish answer is to **keep the refusal and fix the diagnostic**:
say that the literal has no type yet and offer `(2.0 as f32).sqrt()` as a `Fix`
([T-0060](T-0060-machine-readable-diagnostics-fixes-go-with.md)), which turns a confusing
error into a one-keystroke correction. If the default-type rule is wanted instead it is a
language change: a `docs/decisions/D-NNNN` and a §21 row.
