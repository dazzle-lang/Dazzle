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

**Progress, 2026-11-27** (the user's call: model the cases, so correct code keeps compiling,
and refuse only what is unsafe). A survey checked every corpus program and example with each
give-up reported as a warning (a temporary loop in `report.rs` over `a.unchecked` and
`a.incomplete`). It found 155 places in safe code to start with. Modeled since:

- **`Error` and `SourceLocation` fields** (34). Both hold numbers and static text. A real
  hole was found here: `e.message = s.as_str()` let an error carry freed text out of the
  function. A message is static text now (r159, DZ0435).
- **Arrays, structs and union payloads of views** (47). They hold every view their parts
  hold, so `[s.as_str(), "x"]` dies when `s` grows. Before, they held none, and a read after
  the kill was accepted.
- **A plain `T` is never a view** (D-0093), so `pop() -> T?`, `step(acc, c) -> T` and
  `let acc = start` are followed, not unknown (about 30).
- **Values known to hold no view.** Literals, `const`s, `none`, `typeinfo_of`, and calls
  whose view inputs are all such (`"a b".split_whitespace()`) are exact (`Value::exact`).
  That tells them apart from a value the walk lost.
- **Loops.** A by-value `view T` parameter stands for the caller's memory, and its elements
  are followed. A tuple binding is followed as a single one is. A loop whose element was
  lost is now reported whatever its shape: 18 single-binding loops were lost in silence.
- **A loop over a temporary container**, `for &s in make()`, was a second real hole:
  `return s.as_str()` returned text from the freed list. A view binding there now stands
  for the element itself, which dies with the pass (DZ0413).

Left in safe code (about 10 places): `^str` results with `or` (`f.tag_value("json") or
"-"`), a loop over a temporary `String`'s `split_whitespace()`, a tuple a generic call
returns (`s.0`), variadic arguments built into an array (`tell("mixed", c, n, …)`), and a
result through `or unreachable`. Not holes, by §1.1: C and C++ calls (27, vouched by their
recipe) and code inside `unsafe` (22). Once those are modeled, the last step is to make a
give-up outside `unsafe` an error.

## Where the code is

| Piece | Location |
|---|---|
| Where the analysis gives up | `crates/dz_flow/src/ties/body.rs`: every call of `self.unchecked(` and `self.incomplete(`; `BodyAnalysis::unchecked`, `incomplete` |
| What reads them | `crates/dz_flow/src/ties/report.rs` (`incomplete` only); `crates/dz_flow/tests/ties.rs` |
| Values known to hold no view | `Value::exact`; `Analysis::followed`, `aggregate`, `static_inputs`, `holds_views` in `body.rs` |
| `Error`'s message | `Analysis::sets_error_message`, `static_text`; `BodyAnalysis::dynamic_messages`; DZ0435 in `report.rs` |
| Tests | `tests/flow/error_message_static.dz`, `tests/flow/aggregate_views.dz`, `tests/flow/loop_over_temporary.dz` |
