---
id: T-0092
title: dz fmt collapses a multi-line array literal and detaches its elements' comments
milestone: none
status: open
kind: bug
area: tooling
---

**What it is.** An array literal written one element per line, each with a trailing comment,
is joined onto one line, and the comments are moved after the whole declaration, where they
no longer say which element they belong to:

```dz
const CHORDS: [4][4]i32 = [
    [0, 3, 7, 12],                           // A minor
    [-4, 0, 3, 8],                           // F major
    [3, 7, 10, 15],                          // C major
    [-2, 2, 5, 10],                          // G major
]
```

becomes

```dz
const CHORDS: [4][4]i32 = [[0, 3, 7, 12], [-4, 0, 3, 8], [3, 7, 10, 15], [-2, 2, 5, 10]] // A minor

// F major
// C major
// G major
```

The tree is unchanged, so invariant 10 holds in the letter, but the comments now annotate
nothing. Found writing `examples/miniaudio` on 2026-10-24, which put the chord names in a
comment above instead.

**Why it matters.** Tables written as array literals are common in games: palettes, level
data, waypoints, chord progressions. A comment per row is how they are documented, and a
formatter that makes them misleading teaches people not to write them.

**What it would take.** Compiler work only (`crates/dz_fmt/src/lib.rs`). A literal containing
a comment should keep one element per line, with each trailing comment after its element,
the way a struct literal with a comment already stays broken. Related:
[T-0082](T-0082-dz-fmt-moves-trailing-comment-after.md), the same problem after `{` and `}`.
