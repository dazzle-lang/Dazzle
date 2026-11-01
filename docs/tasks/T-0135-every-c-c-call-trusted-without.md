---
id: T-0135
title: Every C and C++ call is trusted without saying so
milestone: none
status: open
kind: design
area: interop
---

Point 6 of [D-0094](../decisions/D-0094-safe-code-has-no-undefined-behaviour.md), which is
open. It revises §15.

**Where the code is**

| Piece | Where |
|---|---|
| The rule | §15, the paragraph "The importer only teaches the compiler what exists…" |
| Imported declarations | `dz_cppimport` |
| A module's `[cpp]` table and its libraries | `dz_project/src/lib.rs` (the module's C and C++ settings, `CppLib`) |
| The check that a call to an `unsafe fn` is in `unsafe` | `sig.is_unsafe` in `dz_check/src/check.rs` |

**What it is.** §15 says "a call into a header is ordinary safe code". A C or C++ function
that keeps a pointer it was given, such as a callback's user data or a buffer a library
reads later, can be handed a reference to a local. When the local drops, the library still
holds the pointer, and nothing in the Dazzle source marks the risk:

```dz
let mut state = State{}
sapp_set_userdata(&mut state)       // safe code today; the library keeps the pointer
```

**Why it matters.** It is the one unchecked door left in safe code once D-0094's holes
close. Every game calls C libraries.

**Why it is this way.** Rust makes every foreign call `unsafe`, which in game code means
`unsafe` around every raylib call. §15 rejects that as noise, rightly. Rust 2024 found a
middle way: `unsafe extern "C" { safe fn sqrt(x: f64) -> f64; }`. Whoever writes the
binding vouches for it, once.

**What it would take.** A choice:

- **A. Trust per header** (proposed). The module importing it says so once in its
  manifest, and lists the functions that keep a pointer, which need `unsafe` to call.
  There is little noise, and the trust is one line `grep` finds.
- **B. A `safe` list per function.** Exact, but long: raylib has over 500 functions.
- **C. A value-only signature is trusted.** A function taking only numbers, structs by
  value and C strings cannot keep a Dazzle pointer, so it is safe without a word. A
  function taking a pointer or reference needs A's or B's vouching. This combines with A.
- **D. Keep §15**, and document C calls as the stated trust boundary.

A language change: §15, every example's manifest, and a §21 row.
