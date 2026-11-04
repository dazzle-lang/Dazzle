---
id: T-0156
title: "A String always allocates: short text kept inside the String, as std::string does"
milestone: M12
status: done
kind: design
area: stdlib
---

A `String` is a `List<u8>` and its closing zero, so every non-empty string allocates.
`std::string` keeps text of up to 15 bytes (libstdc++ and libc++ alike) inside the object
and allocates only for longer text. In `tests/bench_programs/words` every word is under 16
bytes, so each of the 2.9 million `String`s the program builds is an allocation C++ does
not make; regions made each one a pointer bump (T-0117), but not free, and the word count
is still 1.2-1.3 times C++ (2026-11-03, after T-0155).

**Where the code is**

| Piece | Where |
|---|---|
| `String`: `m_store`, the length with its heap bit, the allocator, `tie @text = m_store`; `start`, `set_len`, `make_room`; its `Drop` and `Copy` | `dz_project/src/std/text/string.dz` |
| A tie on plain data names those bytes | `tied_bytes`, `plain_bytes` in `dz_flow/src/ties.rs` |
| A view inside an element dies with the array | where a call's returned view gets its tie, in `dz_flow/src/ties/body.rs` |
| The tests | `tests/run/short_strings`, `tests/flow/view_inside_an_element.dz` |

**The question, for the user.** Inline storage changes `String`'s layout and what its tie
means: a view into a short string points into the `String` itself, so moving the `String`
ends it, where today only growing does. That is the same rule `std::string` has (a move
invalidates pointers into a short string), but it would be new to Dazzle's ties, and every
`kills` clause on `String` would need to say so. Alternatives: a separate `SmallString`
type, or leaving `String` as it is and relying on regions.

**Done** (2026-11-04, r114, [D-0095](../decisions/D-0095-string-keeps-short-text-in-place.md)).
The user chose to keep short text in `String` itself. Text of up to 15 bytes is in place,
and the `String` is still 32 bytes. A view inside an element now dies with the array, so a
view into a list element's text, kept across a push, sort or swap, is refused (§6.7). No
program in the repository was. `words`: the same speed, since in a region an allocation was
already a pointer bump, and a peak of 37.9 MiB, from 40. `docs/log/r81-ties.md` has the
detail.
