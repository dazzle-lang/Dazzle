---
id: T-0046
title: Converting `Mat4` to raylib's `Matrix` copies field by field
milestone: M12
status: open
kind: deferral
area: stdlib
where: `dz_project/src/std/math/`, `Mat4`
---

**What goes wrong.** `std`'s `Mat4` and raylib's `Matrix` hold the same sixteen floats in the
same order in memory, so passing one as the other *should* be a reinterpretation and nothing
more. But raylib's struct declares its fields **one per element, row by row** —
`m0, m4, m8, m12, m1, m5, …` — rather than as an array, so there is no field-to-field
correspondence to cast between: converting means writing out sixteen assignments.

**Why it matters.** It is a per-call cost in the one place it is paid most: a matrix handed
to `DrawMesh`, `rlPushMatrix` or a shader uniform, once or twice per object per frame.
Sixteen loads and stores where there should be none is not catastrophic, but it is exactly
the kind of invisible overhead a language that compiles to C++ should not impose, and the
code is ugly in a way that invites a transcription mistake in one of the sixteen lines.

**What it would take.** Three candidate answers:

1. **A transmuting cast at the boundary**, allowed because the layouts provably match: an
   `unsafe` reinterpretation in the one conversion function, with a static assertion that
   the two sizes and alignments agree so a raylib version that reorders its fields fails to
   compile rather than drawing garbage. Cheapest and correct; needs a way to spell the
   assertion, which is where [T-0071](T-0071-size-alignment-on-hover.md)'s observation
   bites — Dazzle has no layout model of its own and `T::size` is the C++ compiler's answer,
   which is fine for an assertion in generated C++ but means the check lives in the emitted
   code rather than in the checker.
2. **Generate the sixteen assignments** from reflection (§13), so the code is not hand-written
   even if the work still happens. Removes the transcription risk, not the cost.
3. **Leave it**, and document that a matrix conversion is not free. Defensible: a game that
   cares holds its matrices in raylib's shape to begin with.

Option 1 is the one worth building, and the static assertion is the part that makes it safe
rather than a bet on a third-party header.
