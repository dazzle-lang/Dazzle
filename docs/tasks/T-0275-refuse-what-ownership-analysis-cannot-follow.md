---
id: T-0275
title: Refuse what the ownership analysis cannot follow, instead of accepting it
milestone: M19
status: open
kind: design
area: ownership
---

**Found 2026-11-27**, asked whether threads are now safe. The ties analysis (spec §6.7) records
16 kinds of code it cannot follow (`Analysis::unchecked` and `Analysis::incomplete` in
`crates/dz_flow/src/ties/body.rs`), and **nothing reports them**: `BodyAnalysis::unchecked` is
read only by `dz_flow`'s own tests, and `incomplete` only quiets a few other reports. A program
in any of them is accepted. The reasons it records:

- not modelled yet: tuple iterator bindings (`for (a, b) in …`), tuple view destructuring,
  aggregate view provenance, optional view provenance;
- unknown storage: a field it cannot follow, field storage, the indexed element's storage, a
  move, an assignment or a destruction through unknown storage;
- unknown callees: the callee's signature, a callee's tie path that does not resolve, a call
  effect of unknown provenance;
- a loop whose provenance did not converge, a tied pointer's elements trusted, a pointer type
  with no `tie [*]`.

Each one safe code can reach is a possible use after free on one thread, and a possible data
race in a `parallel` block or loop: §1.1 and §22 promise neither happens.

**What it needs.** First, which of the 16 safe code can reach, each with a program (into the
aliasing matrix, T-0274, where it fits). Then the user's call, since it refuses programs that
are correct today: report each as an error (sound by construction, the price being programs
refused until the case is modelled), or as a warning, or model the cases one by one and refuse
only the rest. Inside a `parallel` block or loop, refusing is the conservative minimum.

## Where the code is

| Piece | Location |
|---|---|
| Where the analysis gives up | `crates/dz_flow/src/ties/body.rs`: every call of `self.unchecked(` and `self.incomplete(`; `BodyAnalysis::unchecked`, `incomplete` |
| What reads them | `crates/dz_flow/src/ties/report.rs` (`incomplete` only); `crates/dz_flow/tests/ties.rs` |
