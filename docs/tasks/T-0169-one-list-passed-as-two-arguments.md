---
id: T-0169
title: One list passed as two arguments to a function that keeps nothing across its kills
milestone: none
status: dropped
kind: design
area: ownership
---

Asked for by the user on 2026-11-04 with T-0168. **Dropped the same day, the user's choice
(C):** one list passed as two arguments of a function that kills through both stays refused
(DZ0420). Deciding it from the callee's body would let an edited body break its callers, and
passing the list once does the same work.

**Where the code is**

| Piece | Where |
|---|---|
| DZ0420, a kill reached through two arguments | the `for kill in kills` loop of `invoke` in `crates/dz_flow/src/ties/body.rs`; reported by `crates/dz_flow/src/ties/report.rs` |
| The rule | spec §6.7, D-0094 point 1 |

**What it is.** `grow_both(&mut xs, &mut xs)`, where `grow_both` kills `a@items` and
`b@items`, is refused (DZ0420), as Rust refuses it. The rule is there because the callee is
checked once with its parameters taken as separate: it may hold a view into `a` while it
grows `b`, which dangles when both are `xs`. A callee that holds nothing of one argument
across a kill through another is safe to give the same list twice.

**Why it needs a choice.** Whether a callee holds such a view is a fact about its body, and
the ties check decides every call from the callee's signature alone: a change to a body
never breaks a caller in another module, and each module's result depends only on the
signatures it uses (CLAUDE.md invariant 3). The options:

- **A. Say it in the signature.** A mark on the function, or on the parameters, that it keeps
  no view of one parameter across a kill through another, checked against the body as a
  `kills` clause is (refused where the body does hold one), and written by `dz fix` where it
  holds. Callers then may pass one value as both. New syntax.
- **B. Infer it from the body.** No syntax, but a caller's acceptance then depends on the
  callee's body, so editing a body can break callers elsewhere, the ripple T-0137 is about
  for clauses.
- **C. Leave it.** One list passed twice to a function that kills both is rare, and passing
  it once does the same work.
