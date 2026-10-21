---
id: T-0003
title: A handle's `or` with a value fallback
milestone: M12
status: open
kind: feature
area: ownership
---

**What `or` does on a handle.** `h or …` reaches an `Rc`'s, `Weak`'s or `Box`'s value
(spec §14.5, §9.4). What it produces on the success side is a **reference** — `&T` into the
value the handle holds — because the value stays where it is and is kept alive by a guard
([T-0002](T-0002-skip-guard-count-when-nothing-can.md)). That is the right answer for the
clauses that do not produce a value at all:

```dz
let target = self.target or return      // a Weak: give up if the entity is gone
let t = sprite.texture or continue
```

**What is refused.** A fallback that is a *value* rather than a reference does not type:

```dz
let hp = entity.health or 7             // refused: `&i32` on one side, `i32` on the other
```

A raw pointer is refused for the same reason (`p or 7`), and for the same underlying
reason: there is no `&i32` to hand back for the `7`, because `7` has no storage to refer
to. The workaround spells out the branch and is correct today:

```dz
let hp = if let v = entity.health { v } else { 7 }
```

**Why it matters.** `or` with a fallback value is the shape this reads best in, and the one
a programmer coming from an optional will reach for. `x or default` is already how an
**optional** behaves (`T?`), so the language teaches the form and then refuses it on the two
types that look most like optionals. That inconsistency is the real cost: not that the code
cannot be written, but that the rule for when `or` takes a value is "it depends what is on
the left", which is hard to hold in your head and hard to explain in a diagnostic.

**The design question, for the user.** Three answers, and this wants deciding **for
pointers and handles together** rather than one at a time:

1. **Leave it refused**, and make the diagnostic teach the `if let` form. Simplest, and
   keeps `or` meaning exactly one thing on a handle: *give me the reference or leave*.
2. **Make `or` on a handle produce a value when the fallback is one**, copying out of the
   handle — so `entity.health or 7` is an `i32` and `entity.health or return` is still a
   `&i32`. Convenient, but the result type of `or` would then depend on its right operand,
   and it silently copies, which this language does not do elsewhere (§6.3).
3. **A separate spelling for the copying form**, e.g. a method, leaving `or` alone. No new
   ambiguity, at the cost of one more name.

Option 1 is the cheapest and option 2 the most convenient; the thing that argues against 2
is that it is the only place in the language where an operator's result type is decided by
its right-hand side.

**Either way it is a language change** — it changes what a valid Dazzle program looks like —
so it gets a `docs/decisions/D-NNNN` file, a §21 revision row, and matched cases in
`tests/workspace/typed/` and `typeerrors/` (invariant 8). Option 1 needs only the
diagnostic and its `tests/errors` file.
