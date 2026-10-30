---
id: T-0114
title: An untied pointer becomes a reference only in unsafe
milestone: none
status: open
kind: feature
area: ownership
---

Step 6 of [D-0093](../decisions/D-0093-views-are-borrowed-no-bare-str.md), the last; the
steps are listed in [T-0109](T-0109-text-only-borrowed-str-const-str.md).

**What it is.** `p or return` turns any `^T` into a `&T` safely today (§5.2), and the
reference has no tie: it is how memory the checker never saw enters checked code. After this,
a pointer with a known tie (one a safe call returned with `in`) still converts safely, to a
tied `&T`; an untied one, from a field, a cast or C, converts only in `unsafe`:

```dz
let e = world.enemies.first() or return      // tied: safe, `e` is in world.enemies@items
let w = ctx.world or return                  // error: `ctx.world` is untied
let w = unsafe { ctx.world or return }       // fine: the author vouches for it
```

**Why it matters.** It closes the last hole that needs no `unsafe` to fall into: a reference
from a context struct's `^mut World` (§5.2) aliasing a separate `&mut world`, which §6.7
says the checker cannot see. With it, everything unchecked is written `unsafe`.

**Why it is this way.** The user chose this over keeping the conversion safe as the one
documented hole (D-0093, point 11). The cost is an `unsafe` block wherever a context struct
is used.

**What it would take.** A language change (§5.2, §6.7, a §21 row, revising D-0016/D-0037's
safe conversion as needed). The checker must know whether a pointer value is tied, which is
`dz_flow`'s knowledge, not the type checker's.

**Settled in r90:**

- **The check is `dz_flow`'s**, a new code there, since only it knows a pointer's origin. The
  other way, the type checker treating any pointer not straight from a call as untied, would
  refuse `let p: ^mut Player = &mut world.players[0]` and then `p or return`, which §5.2
  shows as safe.
- **A pointer parameter is untied.** If it were tied to itself, passing `ctx.world` to it
  would be safe, and the hole would only move to the call.
- **The cost is small.** In `examples/`, two conversions of `examples/ownership`'s
  `m_allocator`. In std, `Formatter::m_out` and the allocators' own fields; the map
  iterators' `m_map` become references in view structs first
  ([T-0111](T-0111-view-structs-struct-that-holds-view.md)).

| Piece | Where |
|---|---|
| `or` on a pointer | `or_expr` in `dz_check/src/check.rs` |
| What a pointer's origin is | `Value`, `Origin` in `dz_flow/src/ties/body.rs` |
| Context structs in use | `examples/` (search `^mut`) |
