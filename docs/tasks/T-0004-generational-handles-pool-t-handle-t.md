---
id: T-0004
title: Generational handles: `Pool<T>` and `Handle<T>`
milestone: M12
status: open
kind: feature
area: ownership
---

**What a generational handle is.** A `Pool<T>` stores values in one contiguous array and
hands out a `Handle<T>` that is not a pointer but a pair: an **index** into the array and a
**generation** counter. Every slot carries its own generation; freeing a slot bumps it.
Resolving a handle compares the two:

```dz
let mut enemies = Pool<Enemy>(capacity = 256)
let h = enemies.add(Enemy{ hp: 10 })        // Handle<Enemy> { index: 0, generation: 1 }

enemies.remove(h)                            // slot 0's generation becomes 2
if let &e = enemies.get(h) { e.hp -= 1 }     // none: the handle's 1 != the slot's 2
```

So a handle to something that has died reads as **absent**, not as a crash and not as
whatever happens to occupy that memory now. That is the whole point.

**Why game code wants this, and not the three handles it already has.** §14.5 gives `Rc`,
`Weak` and `Box`, and they are the right tools for *shared ownership* — a texture several
sprites draw. They are the wrong tool for the thing game code does most: a set of entities
or resources that are created and destroyed constantly and referred to from everywhere.

| | `Rc`/`Weak` | `Pool`/`Handle` |
|---|---|---|
| Where the values live | one heap allocation each, scattered | one array, contiguous — iterating them is a linear scan |
| Size of a reference | pointer + a shared control block | 8 bytes, and **copyable, comparable, hashable, serialisable** |
| Cost of a reference | a refcount increment and decrement, plus a guard ([T-0002](T-0002-skip-guard-count-when-nothing-can.md)) | an index compare |
| Dangling | impossible by construction | reads as absent, which is the same safety with no bookkeeping |
| Who decides lifetime | the last owner, implicitly | the pool's owner, explicitly: `remove(h)` |

The last row is the one that matters for a game: an entity dies when the game says it dies,
not when the last system happens to forget about it. And because a handle is a plain
8-byte value with no ownership, it can be stored in a struct field, put in a `List`, used
as a `HashMap` key, sent over a network and written to a save file — none of which an `Rc`
can do well, and the first of which a `&T` cannot do at all (D-0003).

**What it would take.**

1. **`Pool<T>`** — a `List<T>`-backed dense store plus a parallel `List<u32>` of
   generations and a free list of vacated indices. `add`, `remove`, `get`, `get_mut`,
   `len`, `clear`, and iteration over the live slots. Written in Dazzle in
   `crates/dz_project/src/std/collections/`, one declaration per file, listed in
   `STD_SOURCES`.
2. **`Handle<T>`** — `{ index: u32, generation: u32 }`, `Copy`, `Eq`, `Hash`, with a `null`
   handle that is never live. Phantom in `T`: `Handle<Enemy>` must not be accepted where
   `Handle<Bullet>` is wanted, which is the one thing that needs checking works — a type
   parameter a struct does not store a value of.
3. **Decide how a handle is resolved.** The cheap answer is `pool.get(h) -> &T?`, an
   ordinary optional, and nothing new is needed in the language. The tempting answer is to
   implement `Nullable` so `h or …` and `if let v = h` work on the handle itself as they do
   on an `Rc` — but a handle cannot resolve itself, it needs its pool, and `Nullable` has no
   way to say that. **The user's call**, and worth settling before any code: if the answer
   is the pool-method form, this is pure stdlib work with no language change and no §21
   row.
4. **A name.** §14.2 already promises a `Pool<T>` that is an **allocator** (a fixed-size
   block pool, beside `Arena` and `FreeList`). Two different `Pool<T>` in `std` would be a
   mistake. Either this one is named something else (`Store`, `Slots`, `Arena` is taken) or
   the allocator is, and §14.2 is edited to match — in which case there *is* a §21 row, for
   the rename.
5. Tests: a `tests/run` case covering the stale-handle read, reuse of a freed slot, and
   iteration skipping dead ones; a `typeerrors/` case for `Handle<Enemy>` against
   `Handle<Bullet>` (invariant 8).

**Where it sits.** Spec §14.5 is called "Handles: `Rc`, `Weak` and `Box`" and this adds a
fourth kind with different semantics, so it wants either its own subsection or a paragraph
in §14.5 saying when to reach for which — the choice between them is the decision a
programmer actually has to make, and nothing in the spec helps with it today.
