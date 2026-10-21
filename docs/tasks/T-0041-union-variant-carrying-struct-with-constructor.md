---
id: T-0041
title: A union variant carrying a struct with a constructor cannot be built
milestone: M12
status: open
kind: deferral
area: checker
where: `dz_check/src/check.rs`, `union_variant_literal`
---

**What goes wrong.** A tagged `union`'s variant is built with a literal, and the literal for
a variant carrying a struct *is* that struct's literal. A struct that declares a
**constructor** refuses to be built by literal (`DZ0543`) — that is the point of declaring
one. So the two rules meet and the variant becomes unbuildable:

```dz
struct Sound { Sound(id: i32) { … } }

union Event {
    Quit
    Sound(Sound)
}

let e = Event::Sound(Sound{ id: 5 })    // DZ0543: Sound has a constructor
let e = Event::Sound(5)                  // there is no spelling for this
```

The union itself is fine: it compiles, it switches, and it drops its payload correctly
(r47). It is only *constructing* that one shape that has no spelling.

**Why it matters.** Not much yet — the workaround is to drop the constructor and use a
literal, or to wrap the construction in a function — but it is a gap of the worst kind: two
features that each work, composed into something with no way to say it, and a diagnostic
that blames the struct rather than the composition. Someone meeting it has no way to tell
whether they got the syntax wrong.

**The question is the spelling, and it is the user's.** Three candidates:

1. **`Event::Sound(5)`** — the variant's arguments are forwarded to the payload's
   constructor. Shortest, and reads like the union's own constructor; the cost is that
   `Event::Sound(x)` then means two different things depending on whether `Sound` has a
   constructor, and a variant carrying a struct with *several* constructors runs into
   [T-0036](T-0036-several-constructors-are-not-resolved-by.md).
2. **`Event::Sound(Sound(5))`** — build the payload explicitly with its constructor, and let
   a variant literal accept any expression of the payload type rather than only a struct
   literal. Most explicit, no new meaning for anything, and it is probably what the rule
   *should* have been: a variant takes a value, and a struct literal is just one way to write
   one.
3. Leave it refused and improve `DZ0543` to name the union and suggest a helper function.

Option 2 looks like a bug fix rather than a design change — `union_variant_literal` requiring
a *literal* specifically is the arbitrary part — in which case it needs no §21 row and only a
`typed/` case plus a `tests/run` case that drops the payload. Worth confirming that reading
with the user before building it.
