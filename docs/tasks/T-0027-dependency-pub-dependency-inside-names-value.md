---
id: T-0027
title: A dependency's `pub` dependency inside a `[names]` value
milestone: M9
status: open
kind: deferral
area: resolve
where: `dz_project`, `[names]` parsing
---

**What `[names]` is for.** When two opened dependencies both export a bare name, writing it
bare is an error (`DZ0310`, §3.2) until the module chooses one:

```toml
[names]
Vec2 = "std"            # a bare Vec2 is std's, in every file of this module
```

**What is refused.** The value is **a key of this module's `[dependencies]`**, and nothing
else. So a name that lives in a dependency's *public* dependency — which is reachable in
code as `rl::shapes::Rectangle` (§3.2) — cannot be chosen directly:

```toml
[names]
Rectangle = "rl::shapes"     # not a dependency key of this module
```

The workaround is to give it a key first, with `via`, and then choose that:

```toml
[dependencies]
rlshapes = { via = "rl::shapes", open = true }

[names]
Rectangle = "rlshapes"
```

**Why it matters.** Mildly: the workaround is two lines, documented in §3.2, and it arguably
reads better than the one-line form — a thing you refer to twice deserves a name. What makes
it worth a row is the **diagnostic**, which today reports the value as an unknown dependency
rather than saying that a path is not accepted here and that `via` is how to give it a key.
Someone who has just read §3.2's `rl::shapes::Rectangle` example will try the path form
first, because it is the spelling the spec just taught them.

**What it would take.** Either answer is small, and the choice is the user's:

1. **Keep the restriction and fix the message.** `[names]` takes a key; the error should
   recognise a `::` in the value and say "a `[names]` value is a dependency key — give
   `rl::shapes` one with `via` first", with the two lines to add. Cheapest, and it keeps one
   rule: a `[names]` value is a key.
2. **Accept a path**, resolved the same way `via`'s value is — the resolution already exists,
   so this is plumbing rather than new mechanism. The cost is that there are then two
   spellings for the same thing and `[names]` values are no longer uniform.

Option 1 fits "one word, one job" ([T-0070](T-0070-one-word-one-job.md)) better. Either way,
a `tests/workspace` fixture with a public dependency and a name conflict pins it, and the
error case belongs in the manifest-diagnostics corpus rather than `tests/errors` since it is
not a `.dz` mistake.
