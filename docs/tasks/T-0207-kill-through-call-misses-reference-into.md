---
id: T-0207
title: A kill through a call misses a reference into a multi-place tie's later place
milestone: none
status: open
kind: bug
area: ownership
---

Found on 2026-11-12 while writing the sparse-set ECS example (`examples/ecs`), beside T-0206.
Safe code, no `unsafe`, writes freed memory.

**What it is.** When a struct declares a tie standing for several places (§6.7, "A tie may
stand for several places"), a reference into any place but the first survives a kill that
comes through a call's clause:

```dz
struct W {
    pub a: List<i32>
    pub b: List<i32>
    tie @lists = a@items, b@items
    W() {
        a = List<i32>()
        b = List<i32>()
    }
}
fn grow_b(w: &mut W) kills w.b@items {
    w.b.push(1)
}
fn missed(w: &mut W) kills w.b@items {
    let r = &mut w.b[0]
    grow_b(w)
    r += 1        // wrong: no DZ0413
}
fn caught(w: &mut W) kills w.b@items {
    let r = &mut w.b[0]
    w.b.push(1)
    r += 1        // DZ0413, as it should be
}
```

It is the same whether the clause names the field path (`kills w.b@items`) or the tie
(`kills w@lists`), and whether the call is a method or a free function. A reference into the
*first* place (`w.a`) is caught. A struct whose tie names one place, or that declares none,
is not affected.

In the ECS: `World` has `tie @components = positions@items, velocities@items,
health@items`, so `let h = world.health.get_mut(k) or unreachable`, then `world.spawn(…)`
(`kills self@entities, self@components`), then `h.hp -= 1` compiles; so does an explicit
`let q = query(&mut world.health)`, `world.spawn(…)`, `q.valid()`. Reordering the tie to put
`health` first makes those caught and moves the hole to `positions`.

**Why it happens.** Both sides are made canonical (`canonicalize`: every path a tie stands
for is rewritten to its first path), but in different frames. The callee's clause is
normalised in the callee's scope, rooted at `W`, so `w.b@items` becomes `w.a…`. The
reference's tie comes from `List::index`'s result, normalised in `List`'s scope, and bound
onto the caller's `w.b`. In `invoke`, neither a bound kill nor a bound result origin is
passed through `Cx::canonical` from the caller's root, so the kill is `w.a…`, the reference
`w.b…`, and `Place::covers` never matches. A direct `w.b.push(1)` works because its kill is
bound in `List`'s frame too, so both are spelt `w.b…`.

The reverse holds as well, though it frees nothing: §6.7 says the places of one tie "live
and die together", so a kill through `w.a` should end a view into `w.b`, and a direct
`w.a.push(1)` does not.

**What it would take.** Canonicalise in the caller's frame what `invoke` binds: each kill
place before it is applied, and each result origin's `target` and `tie`, with `Cx::canonical`
(`root_ty` of the place's local), as field reads already are. Then check that the clause
comparison (`DZ0415`/`DZ0416`) and `DZ0420`'s "reached through one argument only", which
compare bound places with `followed_by`, still agree with the canonical spelling.

Tests: a `tests/flow` file with `missed` above, the same through a tie-named clause and
through a method, the reverse direction, and a reference into the first place; a
`dz_flow/tests/ties.rs` case for the bound kill and result being canonical; std and
`examples/` still checking clean (`HashMap`, `SlotMap` and `HashSet` have multi-place ties).

## Where the code is

| What | Where |
|---|---|
| Canonical spelling of a tie path | `canonicalize`, `rewrites`, `canonical_place` in `crates/dz_flow/src/ties.rs` |
| A clause's path, callee-rooted | `normalize`, `Path::bind` in the same file |
| Where a call's kills and result are bound | `Cx::invoke`, `crates/dz_flow/src/ties/body.rs` (the `effects.kills` loop and the `ReturnTie::Explicit \| Elided` arm) |
| Caller-frame canonicalisation | `Cx::canonical` in the same file |
| Whether a kill ends a reference | `Origin::ended_by`, `Place::covers` |
| Applying a kill | `Cx::kill`, `Cx::kill_places` |
