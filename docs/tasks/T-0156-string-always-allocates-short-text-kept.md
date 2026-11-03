---
id: T-0156
title: "A String always allocates: short text kept inside the String, as std::string does"
milestone: M12
status: open
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
| `String`: `m_bytes: List<u8>`, the closing zero, `tie @text = m_bytes` | `dz_project/src/std/text/string.dz` |
| What a `String` hands out: `as_str`, `as_cstr` (`-> &cstr in self@text`) | the same file |

**The question, for the user.** Inline storage changes `String`'s layout and what its tie
means: a view into a short string points into the `String` itself, so moving the `String`
ends it, where today only growing does. That is the same rule `std::string` has (a move
invalidates pointers into a short string), but it would be new to Dazzle's ties, and every
`kills` clause on `String` would need to say so. Alternatives: a separate `SmallString`
type, or leaving `String` as it is and relying on regions.
