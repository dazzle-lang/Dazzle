---
id: T-0275
title: Refuse what the ownership analysis cannot follow, instead of accepting it
milestone: M19
status: done
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

**The rest of the safe-code cases, 2026-11-28.** No give-up is left in safe code in the
corpus or the examples:

- **An element indexed through a renamed tie.** `m_books[i]` in a struct that declares `tie
  @items = m_books@items` lost every view, because `index`'s result is spelt by the
  canonical tie and was compared with the uncanonical field. This was a third real hole:
  `let t = m_books[0].as_str()` kept across `m_books.push(…)` was accepted. The two are
  compared canonically now, and an index result no parent covers keeps the views it has.
- **A reference to a temporary that holds no view** (`&Cat{ name: "tom" }` given to a
  variadic `..&Describe`) holds none either. It can only be passed on, since DZ0407 refuses
  keeping one.
- **A call whose signature's result holds no view** (`ReturnTie::None`) returns an exact
  value. That covers a plain-`T` tuple, `swap<i32, const &str>(…)`, since DZ0517 already
  refuses a view for a plain `T`. A tuple part of an exact tuple is exact.
- **A loop over an iterator of a temporary**, `format!(…).split_whitespace()`, or over a
  temporary iterator that owns its items. These are followed as a temporary container's
  elements are, so returning a word is refused (DZ0413).
- **An exact value never owns memory.** A `String` holds no view, but a view into it dies
  with it. Without this rule a `format!` temporary read as static.

Not holes, by §1.1: C and C++ calls (27 places, vouched by their recipe) and code inside
`unsafe` (22).

**Done, 2026-11-28 (r160).** A give-up in safe code is now an error, DZ0436
(`BodyAnalysis::unfollowed`, filled by `Analysis::refusable`). It is not raised inside
`unsafe`, at a call to C or C++ (`kills_unknown`, `Analysis::foreign`), in `std`'s bodies,
or where another error already refuses the same place. Making it an error first showed more
cases in `tests/flow` and in `std`, which the corpus survey had not covered. These were
modeled, not refused:

- A view field of a view struct (`&p.items[p.at]`) is in the field the struct's tie names.
- A view struct's constructor (`Split(text, separator)`) is in every view input.
- A literal root of an `in` (`split(",")`) adds nothing.
- A kill through a temporary (`make().push(x)`) reaches nothing kept.
- A raw pointer from an allocator, which only `unsafe` can use, is not refused.

**A fourth real hole was found in `std` itself**: `split` said `in self`, but its `Split`
holds the separator too. A split kept while the separator's `String` grew read freed memory
and split wrongly. It is `in (self, separator)` now. `std` keeps one trusted give-up,
`SliceIterMut::part`, which builds its result from an `unsafe` slice.

Tests: `tests/flow/temporary_receiver.dz` (accepted), `split_keeps_separator.dz`. DZ0436
has no example program in `dz explain`, since none in the corpus reaches it: it is the
backstop for a shape nobody has modeled yet.

## Where the code is

| Piece | Location |
|---|---|
| Where the analysis gives up | `crates/dz_flow/src/ties/body.rs`: every call of `self.unchecked(` and `self.incomplete(`; `BodyAnalysis::unchecked`, `incomplete` |
| What reads them | `crates/dz_flow/src/ties/report.rs` (`incomplete` only); `crates/dz_flow/tests/ties.rs` |
| Values known to hold no view | `Value::exact`; `Analysis::followed`, `aggregate`, `static_inputs`, `holds_views` in `body.rs` |
| `Error`'s message | `Analysis::sets_error_message`, `static_text`; `BodyAnalysis::dynamic_messages`; DZ0435 in `report.rs` |
| Temporaries in a loop head | `Analysis::over_temporary`; `temporary`, `static_subject` in the `For` arm of `stmt` |
| The refusal | `Analysis::refusable`, `foreign`; `BodyAnalysis::unfollowed`; DZ0436 in `report.rs` (the `std` and already-refused filters) |
| Tests | `tests/flow/error_message_static.dz`, `aggregate_views.dz`, `loop_over_temporary.dz`, `index_through_tie.dz`, `temporary_receiver.dz`, `split_keeps_separator.dz` |
