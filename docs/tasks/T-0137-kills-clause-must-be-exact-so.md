---
id: T-0137
title: A `kills` clause must be exact, so a change in a body ripples up every caller
milestone: none
status: open
kind: design
area: ownership
---

Not part of [D-0094](../decisions/D-0094-safe-code-has-no-undefined-behaviour.md). This is
about keeping Dazzle freer than Rust once it is as safe, and it needs a decision file of
its own.

**Where the code is**

| Piece | Where |
|---|---|
| The rule: a clause names exactly what dies | §6.7, the first bullet under "A `kills` clause"; D-0091 |
| `DZ0415` (a kill left out), `DZ0416` (one that never happens), `DZ0417` (one wider than what dies) | `Mismatch` in `dz_flow/src/ties/report.rs` (`TooBroad`, ~276) |

**What it is.** A `kills` clause must name exactly what its body kills. Naming more is
`DZ0417`, naming less `DZ0415`. So a change inside a body changes its signature, and the
signature of every caller whose clause covered it, all the way up to the function that owns
the memory. Adding `self.particles.push(p)` deep inside `update` changes the clause of
`update`, then of `tick`, then of `run_frame`. `dz fix` writes them, but every signature on
the way changes, much as Java's checked exceptions did.

```dz
pub fn update(&mut self, dt: f32) kills self.bricks@items   // today: exact
pub fn update(&mut self, dt: f32) kills self                // DZ0417: wider than the body
```

The second line is what a library author wants for a stable `pub` API: the most the
function may ever kill. A trait method's clause already works that way (§6.7: an
implementation "may name no more than the trait's").

**Why it matters.** It is the main day-to-day cost of ties, and it grows with the size of
a program. A large program, not a small one, decides whether ties feel light.

**Why it is this way.** Exactness keeps a caller's references alive across any call that
does not really kill them, and keeps the editor's clause hint truthful.

**What it would take.** A choice:

- **A. A `pub` function's clause may be wider than its body**: a promise, held one way (the
  body kills no more than the clause says). Private functions stay exact, where exactness
  is cheap. Callers lose references the call does not really kill. `DZ0417` becomes a
  warning on `pub` functions, or goes.
- **B. Private functions' clauses are inferred, not written.** They are shown as an inlay
  hint and required only on `pub` functions, as a signature's types are. Reading a private
  call then needs the editor to see what it kills, which changes §6.7's "every function
  says it, private ones included".
- **C. Both.**
- **D. Neither**: exact everywhere, with `dz fix` keeping the churn cheap.

Measure before deciding: [T-0138](T-0138-game-ten-thousand-lines-test-ties.md)'s game
shows how often a clause changes in practice.
