---
id: T-0158
title: A move copied its copyable fields, and move() dropped its source twice more
milestone: M15
status: done
kind: bug
area: codegen
---

Found 2026-11-03 writing the playground's ownership example: a `Level` with `String` and
`List` fields, a `drop` that printed and a `copy`, printed two drops of an empty `Level`
at `let c = move(a)`, and a third when `a`'s scope ended.

Two causes, both in the C++ codegen writes for a move:

- **A move copied.** The move constructor `move_members` writes for a type with a `drop`
  or a `copy` of its own assigned each copyable field from `other.f`, a C++ copy: a
  `String` or a `List` field was copied in full, then reset, on every move. A field whose
  type has a `copy` of its own ran it.
- **`move(x)` dropped its source twice more.** `::dz::take`, which a `move` of a copyable
  value lowers to, moved out and then assigned `x = T{}`: for a type whose move constructor
  already resets its source, that ran `drop` on the moved-from value and again on the
  temporary, before the one drop at the end of its scope that spec §6.5 describes.

**Where the code is**

| Piece | Where |
|---|---|
| The move constructor: every field moved (`static_cast<decltype(f)&&>`), never copied | `move_members` in `dz_cpp/src/lib.rs` |
| `take`: the reset only for a type with C++'s own move, which does not reset; `resets_itself` tells them apart by `reset_to_defaults__` | `crates/dz_cpp/src/runtime.rs` |
| Test: a copy runs `copy` once, a move none, and a moved-from value is dropped once | `tests/run/moves` (in `WARNING_CLEAN`) |

**Done 2026-11-03.** The test prints `copy 0` and two extra `drop '' 0` lines with the
compiler before the fix, and neither after, on clang++ and g++. No other program's output
changed.
