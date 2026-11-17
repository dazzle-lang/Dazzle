---
id: T-0239
title: A kills clause the checker suggests names Box's private m_ptr instead of its @value tie
milestone: M15
status: done
kind: bug
area: diagnostics
---

Found 2026-11-17 writing `ecs`'s library (T-0172). `World::despawn` calls `remove` on every
storage, each held in a `Box<AnyStorage>` in `m_storages`. The kill it makes was reported as
`DZ0415`, "`despawn` kills `self@components.m_ptr[*]`", with the help "the body needs `kills
self@components.m_ptr[*], self@entities`".

`m_ptr` is `Box`'s private field. `Box` declares `tie @value = m_ptr[*]` so that a clause
outside `std` can name what is in a box (D-0094 point 3), and the clause that works is a
tie of the world's own through it, `tie @stored = m_storages[*]@value`, then `kills
self@stored`. The message should name the path a caller can write: through a type's tie
where the field is private to another module, as the help already does for a private field
of the function's own type ("have the type declare a tie for it and name the tie").

| What | Where |
|---|---|
| The kill path rendered for `DZ0415`'s message and help | `crates/dz_flow/src` (the `kills` checks) |
| `Box`'s tie | `crates/dz_project/src/std/memory/box.dz` |
| The case | `tests/bench_programs/ecs/libs/ecs/src/world.dz`, `despawn` |

**Done** (2026-11-17). `tie_path_held` (`dz_flow/src/ties/report.rs`) renders a place
through the ties each type declares. It had three faults:
- It took the first tie whose first path matched any prefix, so `@components` won over
  `@stored`. It now takes the one naming the most.
- It did not look through a pointer for ties, so after a `List`'s elements, `Box`'s `@value`
  was never tried. It now does, as `field_of` does.
- It compared a tie's path as declared against a place in canonical form, so `m_storages[*]`
  never met the place's `m_storages` storage. Named ties are now canonicalized first.

A tie that names memory (`@items`) ends a path (`DZ0562`), so it is used only for all that is
left. One that names values (`@value`, a type's `[*]`, written `[*]`) may be followed into
fields. So `despawn` now says `kills self@stored`, or without that tie, `self.m_storages[*]@value`
with the offer `tie @storages = m_storages[*]@value`, which compiles. A field through a `Box`
reads `m_bags[*]@value.items@items`; it read `m_bags[*].m_ptr[*].items@items`. Test:
`crates/dz/tests/fix.rs`, `a_clause_through_a_box_names_its_tie_not_its_private_field`.
