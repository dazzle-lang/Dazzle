---
id: T-0135
title: Every C and C++ call is trusted without saying so
milestone: none
status: done
kind: design
area: interop
---

**Done** (r111, 2026-11-03). The spelling the user settled on that day is one key in the
recipe, `keeps = […]`, which is itself the vouching. The user's point was that the danger
is a few specific functions, so the key names those, and `keeps = []` says there are none.
Struct-by-value parameters count as values only if they hold no pointer, checked
recursively, and a C++ receiver or constructor counts as a pointer. The editor quick fix
that would write `keeps = []` is not built. Build log: `docs/log/r81-ties.md`.

Point 6 of [D-0094](../decisions/D-0094-safe-code-has-no-undefined-behaviour.md), decided
on 2026-11-03 (r107) as options A and C below together: a header is trusted once in the
importing module's manifest, which lists the functions that keep a pointer (`unsafe` to
call), and a function taking only values is safe without a word. It revises §15.

**Where the code is**

| Piece | Where |
|---|---|
| The rule | §15.1's `keeps` bullet, and §15.2's paragraph "The importer only teaches the compiler what exists…" |
| Which parameters are values; the names `keeps` uses | `Header::takes_pointer` and `Header::function_names` in `dz_cppimport/src/lib.rs` |
| `keeps` read from a recipe, checked against the header, and carried to the module | `CppLib::keeps`, `cpp_section`, `unknown_keeps`, `Trust` and `add_foreign` in `dz_project/src/lib.rs` |
| An imported function marked `unsafe`, and why | `foreign_unsafe` (with `collect_foreign`, `foreign_methods`) and `FnSig::foreign_unsafe` in `dz_check/src/lib.rs` |
| The refusal, `DZ0528` with a help that names the recipe | `require_unsafe_call` in `dz_check/src/check.rs`, at every call path and the constructor call |
| Tests | `a_function_taking_only_values_hands_c_no_pointer` in `dz_cppimport/tests/import.rs`; `keeps_vouches_for_the_calls_that_take_a_pointer` in `dz/tests/run.rs`; `tests/run/cinterop`'s `mini_watch` |

**What it is.** §15 says "a call into a header is ordinary safe code". A C or C++ function
that keeps a pointer it was given, such as a callback's user data or a buffer a library
reads later, can be handed a reference to a local. When the local drops, the library still
holds the pointer, and nothing in the Dazzle source marks the risk:

```dz
let mut state = State{}
sapp_set_userdata(&mut state)       // safe code today; the library keeps the pointer
```

**Why it matters.** It is the one unchecked door left in safe code once D-0094's holes
close. Every game calls C libraries.

**Why it is this way.** Rust makes every foreign call `unsafe`, which in game code means
`unsafe` around every raylib call. §15 rejects that as noise, rightly. Rust 2024 found a
middle way: `unsafe extern "C" { safe fn sqrt(x: f64) -> f64; }`. Whoever writes the
binding vouches for it, once.

**What it would take.** A choice:

- **A. Trust per header** (proposed). The module importing it says so once in its
  manifest, and lists the functions that keep a pointer, which need `unsafe` to call.
  There is little noise, and the trust is one line `grep` finds.
- **B. A `safe` list per function.** Exact, but long: raylib has over 500 functions.
- **C. A value-only signature is trusted.** A function taking only numbers, structs by
  value and C strings cannot keep a Dazzle pointer, so it is safe without a word. A
  function taking a pointer or reference needs A's or B's vouching. This combines with A.
- **D. Keep §15**, and document C calls as the stated trust boundary.

A language change: §15, every example's manifest, and a §21 row.
