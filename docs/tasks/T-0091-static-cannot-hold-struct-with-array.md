---
id: T-0091
title: A static cannot hold a struct with an array field that has no default
milestone: none
status: open
kind: design
area: language
---

**What it is.** A `static` needs a constant initialiser (§7.7), and a struct literal must
name every field that has no declared default (`DZ0512`, §6.2). A fixed array has no
constant literal shorter than writing every element, and no repeat form (§5.3), so a struct
holding one cannot be a static at all:

```dz
struct Fountain {
    m_parts: [4096]Particle
    m_next:  usize
}

static APP: Fountain = Fountain{}    // DZ0512: `m_parts`, `m_next` not initialised
static APP: Fountain                 // DZ0201: expected `=`
```

The same struct as a local is fine, `let mut f: Fountain`, since every value is initialised
to its defaults (§6.2). Only the static spelling is missing.

**Why it matters.** C libraries that run the frame loop themselves call back with no
arguments: sokol's `init_cb` and `frame_cb`, GLUT, many audio APIs. The natural home for the
state those callbacks share is a static, and that state is often a fixed pool such as particles,
entities or audio voices. Found while writing `examples/sokol` (2026-10-24), which used
sokol's user-data callbacks instead. That works, but costs a pointer cast and an `or` in every
callback.

**Why it is this way.** Two rules, each reasonable alone: a literal names its fields so a
new field cannot be forgotten silently, and a static's initialiser is constants so C++'s
unordered initialisation cannot bite (D-0046).

**What it would take.** A language change, so a §21 row and a decision. Options:

1. **`static APP: Fountain` with no initialiser** means "every field at its default", as
   `let f: Fountain` does. Smallest change; it is constant by construction when every
   default is. A field whose default is not constant would still be refused.
2. **Fields without a declared default may be omitted from a literal** when their type's
   zero is its default. Broader: it changes every struct literal, not only statics, and
   weakens the "a new field cannot be forgotten" rule.
3. **A repeat form for arrays**, which §5.3 rejects because `;` is a terminator.

Option 1 looks like the cheapest that solves it; it is the user's call.
