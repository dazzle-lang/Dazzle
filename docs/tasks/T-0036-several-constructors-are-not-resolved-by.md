---
id: T-0036
title: Several constructors are not resolved by argument
milestone: M12
status: open
kind: deferral
area: checker
where: `dz_check/src/check.rs`, `call`
---

**What goes wrong.** A struct may declare more than one constructor, but a constructor call
takes **the first one declared**, whatever the arguments say:

```dz
struct Rect {
    Rect(w: f32, h: f32) { … }
    Rect(size: Vec2)     { … }
}

let r = Rect(Vec2{ x: 1, y: 2 })        // resolves to the first, then fails on the arguments
```

The same happens in a **base initialiser**, `: Base(…)` — the same limitation the
constructor call already had, which inheritance (r30) made visible in a second place.

**Why it is this way.** Constructors are the one place the language has something that looks
like overloading, which D-0005 keeps out of the language until a later version. `call` was
written to find *a* constructor by name rather than to choose among several by argument
type, and nothing needed more until a struct had two.

**Why a game needs it** (from the readiness review, 2026-09-14). Two constructors for one
type is the ordinary case in game code — a `Rect` from width and height or from a size
vector, a `Color` from three channels or four, a `Mat4` from a quaternion or from Euler
angles. Today the second one is unreachable and has to become a named function
(`Rect::from_size`), which is a workable convention but not what the declaration says.

**What it would take.** `call` would score each candidate constructor against the argument
list the way a method's overload set is scored, and report an ambiguity rather than silently
taking the first. The interesting questions are what counts as a match (D-0005's reasons for
refusing overloading generally apply here too: no implicit numeric conversions means scoring
is nearly exact-match, which keeps it predictable), and whether a constructor's **default
arguments** take part — `Rect(w, h)` and `Rect(size, scale = 1.0)` could both accept two
arguments. Deciding that is the user's call, and it is small enough to settle in the task
rather than a decision file unless the answer changes what a valid program looks like.

Related: [T-0035](T-0035-generic-base.md),
[T-0041](T-0041-union-variant-carrying-struct-with-constructor.md) (a union variant
carrying a struct with a constructor cannot be built at all).
