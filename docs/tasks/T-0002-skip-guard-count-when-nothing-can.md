---
id: T-0002
title: Skip the guard's count when nothing can drop the owner
milestone: M12
status: open
kind: feature
area: ownership
---

**What the guard is for.** Reaching an `Rc`'s or `Weak`'s value with `or` or `if let` must
not hand out a reference that something could then free underneath it, so spec §14.5
promises the opposite: *while the value is used, it cannot be freed*. The compiler keeps
that promise by taking a **strong `Rc` of its own** for the duration. The checker builds a
`_guard` local holding `h.guard()` and reads the target out of it with `ptr()`
(`nullable_guard`), and `dz_cpp` emits that local as a line of the enclosing C++ block
(`ExprKind::Guarded`), so it is constructed before the reference is used and destroyed when
the block ends:

```dz
if let &mut t = sprite.texture {        // _guard = sprite.texture.guard()  -> count + 1
    sprite.texture = null               // frees nothing: the guard still holds a share
    t.width = 64                        // safe
}                                       // _guard dropped                   -> count - 1
```

**What is wasted.** That is one increment and one decrement per `if let` or `or`, on every
pass, whether or not anything in the body could have freed the value. In the common case
nothing can:

```dz
if let &t = sprite.texture { draw(t.id, x, y) }   // t is read; the handle is untouched
```

Here the count goes up and straight back down for nothing. In a draw loop over a few
thousand sprites that is a few thousand pairs of pointer-chasing read-modify-writes on a
shared cache line, which is exactly the kind of cost a game notices and exactly the kind
this language promises not to impose silently.

**When it is safe to leave out.** The guard is needed only if the value could lose its last
other owner inside the body. It cannot if, for the whole of the body:

- there is **no assignment to the handle itself**, nor to any place it could be reached
  through (`sprite.texture = null`, `*p = null`, `sprites.clear()`);
- there is **no call the compiler cannot see into** — no call at all is the easy version,
  and a call to a function whose body is known not to touch a handle to this value is the
  version that actually helps;
- the handle is **not moved out of**, and nothing in the body drops a value that could
  transitively own it.

The first is cheap to compute: `dz_flow` already walks assignments and moves per block. The
third needs a may-own relation over types. The middle one is the whole difficulty: it is an
interprocedural question, and "no calls in the body" is restrictive enough that the loop
above — which calls `draw` — would still pay. A useful first step is therefore the
conservative one plus a whitelist: no assignment, no move, and only calls to functions that
take no handle of the relevant type and are in this project (so their bodies are available).

**Since ties (§6.7, r81–r88), the middle condition has an answer in signatures.** A call can
drop the value only through a place its `kills` clause names, since dropping or writing a
place kills every tie under it, and a call with no clause kills nothing. So the guard is
needed only when something in the body kills the handle's place: an assignment, a move, or
a call whose clause covers it. A C call kills everything its `&mut` arguments reach, and a
static changes only in `unsafe`, so neither hides a drop. That needs no whitelist of
project functions; it is `dz_flow`'s kill set for the body, read at the `if let` or `or`.
(Noted 2026-10-30, r90.)

**A compiler detail, not a language change.** §14.5's promise is unchanged either way; this
is only about not paying for it where it cannot be observed. So **no §21 row** — it belongs
in `docs/log/m12.md` — and it needs a test that pins the *behaviour* (the value still
outlives the body in every case §14.5 shows) plus a codegen snapshot showing the guard gone
in the elidable case, so the optimisation cannot silently stop applying.

Related: [T-0003](T-0003-handle-or-with-value-fallback.md),
[T-0004](T-0004-generational-handles-pool-t-handle-t.md).
