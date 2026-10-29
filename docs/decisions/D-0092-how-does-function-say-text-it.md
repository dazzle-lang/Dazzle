---
id: D-0092
title: How does a function say the text it returns points into nothing that dies?
status: decided
---

**Decided: `const str`, a type** (r87, 2026-10-29, by the user). Text in static storage — a
literal's, a constant's — is a `const str`. It is the same `{ ptr, len }` as a `str` and
reads as one wherever a `const str` is not wanted, but a `str` never becomes one. Nothing
kills it, so it carries no tie, can be returned from anything, and can be held in a field
or a container. Only `str` may follow `const` for now (`DZ0569`); `const []T` waits for a
program that needs static tables. Spec: §5.3, §6.7, §20.

**The question.** `str` is a view (§5.3): `fn describe(&self) -> str` may return a view of
`self`'s own text, so by §6.7's elision its result is tied to `self`. When every
implementation returns a literal, that is wrong in a way that matters: `fn heaviest<T:
Weigh>(a: T, b: T) -> str { return a.describe() }` is refused, since `a` dies as it
returns, and a state's name cannot be held across a change to the state. A signature had no
way to say the text points into nothing that dies.

**Options considered** (T-0107, 2026-10-29):

- **A. `-> str in static`**, a root for a tie path. Small, but only where a signature has an
  `in`: a field could still not hold such text.
- **B. Elision**: no reference input means static. Useful beside A or C, not instead.
- **C. A type for static text.** Proposed as a separate type and first set aside as heavier;
  the user's spelling, `const str`, made it the choice: a type composes — fields, lists,
  results, parameters — and is checked as a type, with no flow analysis.
- **D. Return a `String`.** An allocation per call, for text that already exists.
- Also raised: text fixed per type as a constant a trait requires (`const NAME: str`). It
  covers part of the need, not a `switch` choosing among literals.

**Resolution.** C, spelt `const str`. A string literal is a `const str` wherever one is
wanted, as it is a `cstr` (§5.3); a `let` with no type keeps a `const str` value's type, and
a literal there is still a `str`, so `let mut label = "none"` can later take any text. Every
implementation for `str` applies to `const str`.
