---
id: T-0215
title: A loop writing through &mut List reloads the length after every store: hoist a slice view where no kill can happen
milestone: M15
status: done
kind: feature
area: codegen
where: `crates/dz_flow/src/views.rs`; `crates/dz_cpp/src/lib.rs`: `loop_with_views`, `view_call`
---

**Done** (2026-11-14). Left by [T-0214](T-0214-inline-did-not-reach-instances-another.md).

| Piece | Where |
|---|---|
| Which loops view which lists: `loop_views`, `view_call` | `crates/dz_flow/src/views.rs` |
| The view before the loop, and `xs[i]`, `xs.len()` read through it | `Emitter::loop_with_views`, `Emitter::view_call` (from `call`) in `crates/dz_cpp/src/lib.rs` |
| Which loops it is asked for | `Emitter::in_body` computes it per body; `stmt_inner` sends a loop with views to `loop_with_views` |
| Tests | `crates/dz_flow/tests/views.rs` (one function per rule); `a_viewed_list_is_still_bounds_checked` in `crates/dz/tests/run.rs` |

**What was wrong.** With every body visible (T-0214), `dst[i] = src[i] * 3` over a
`&mut List<i32>` and a `&List<i32>` still took 41 µs per 100 000 elements on clang++ 19,
against 6 µs for the same loop over two slices. `List::index_mut` checks `i` against
`m_len`, and after each store into the array C++ cannot prove `m_len` and `m_data`
unchanged (with `-fno-strict-aliasing` an `i32` store may alias anything), so it loaded
both again, checked again, and did not vectorize.

**What it does.** Dazzle can prove what C++ cannot: a view of `xs@items` stays good until
something kills `xs@items`, and every call says what it kills (spec §6.7). `views::loop_views`
asks the tie analysis of a body (`analyse_body`) and, for each `while` and `for`, picks the
lists it indexes that

- are a local holding the `List`, or a parameter holding it or a reference to it (a reference
  bound in the body is left out: kills through it are recorded against what it refers to);
- are declared outside the loop;
- have no kill recorded against them anywhere inside the loop, its condition or subject
  included, and the loop has no `incomplete` finding (memory the analysis could not follow).

The outermost loop that qualifies takes the view. `List`'s own bodies are left out whole:
they write `m_len` and `m_data` directly, which changes the length without a kill, and no
other code can, the fields being private (§4).

Codegen then opens a block around the loop, builds `::dz::rt::Slice<T>` (`Slice<const T>`
when the loop only reads) from the list's `m_data` and `m_len`, and emits `xs[i]` as
`::dz::rt::index(view, i, loc)` and `xs.len()` as `view.len`. The slice is built from the
fields, not by `as_mut_slice()`, which an instance defines only if the program calls it
(T-0199). **Every index is still checked**, against the same length. A failing one now
panics as a slice's does, `index 3 is out of bounds for length 3` at the line of the index,
where `List`'s own check said `… for a List of length 3` from inside `std`.

It is not a language change: nothing a program computes changes, and the spec does not fix a
panic's wording.

**Measured** on 100 000 `i32`s, `dz build --release`, same `dz` with the hoist on and off,
`build/` cleared between (µs per pass, three runs):

| Loop | clang++ 19, off | on | g++ 14, off | on |
|---|---:|---:|---:|---:|
| `dst[i] = src[i] * 3`, `&mut List` and `&List` | 40.6–41.6 | 5.6–5.9 | 40.6–40.8 | 41.3 |

For g++ the loops were built with `-falign-loops=64` through a wrapper, since unaligned its
numbers moved by up to 50% between builds of the same code. g++ vectorizes neither form, so
it gains nothing; it is not slower either. Reads through `&List` were already as fast as a
slice after T-0214, since nothing in such a loop stores.

**What it costs to build.** The tie analysis runs again, during emission, for each body
that indexes a list inside a loop. Emitting `words` went from 17.1 ms to 19.7 ms; `expr` and
`astar` did not move. The whole-program benchmarks' run times did not move either: none of
them writes through a `List` in a loop in its hot path.

**Found on the way.**

- g++ stores the `SourceLocation` of `::dz::rt::index`'s panic to the stack on every pass,
  before the compare, rather than on the failing path. That is every checked slice index
  under g++, not only these; `List`'s check never showed it because its panic helper takes
  no location.
- `for i in a..b` emits `i < b`, so `b` is evaluated on every pass: `for i in 0..xs.len() {
  xs.push(…) }` never ends. The spec does not say whether a range's bounds are read once.
  For a viewed list `xs.len()` is the view's length, which cannot differ, since nothing in
  the loop can change it.

**Left.** A kill of anything under the list ends its view, so a loop that changes an
element's own memory, `names[i].push_str("!")` on a `List<String>`, is not viewed, though
that kill leaves the array where it is. A `Deque` or `HashMap` indexed in a loop is not
viewed either.
