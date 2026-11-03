---
id: T-0153
title: "#[inline] is implemented: a function's body in its module's header"
milestone: M15
status: done
kind: feature
area: codegen
---

Each module is its own C++ translation unit, and every non-generic function's body went in
the module's `.cpp`. So a call from the program into `std`'s small functions could never be
inlined, however often it ran: a profile of `tests/bench_programs` (2026-11-03, `gprof`)
showed `String::push` called 14 million times in `words`, `hash_combine` 11 million in
`astar`, and `Allocators::Global()` 32 million in `words`, the last from every move of a
`String` resetting its source to the default allocator (spec §6.4), 25 million of them in
the sort's swaps. Spec §12.8 named `#[inline]` but it was `DZ0316`, not built.

**Where the code is**

| Piece | Where |
|---|---|
| The attribute accepted on a function or method (`AttrTarget::Method`), refused with no body to show (`inline_needs_body`) | `attributes`, `inline_needs_body` in `dz_resolve/src/walk.rs` |
| Which bodies are `#[inline]`: by file and name span, the span a checked body carries (`inline_fns`) | `dz_cpp/src/lib.rs` |
| A marked body captured into `inline_defs` with `inline` (`emit_body`, `is_inline`), written into the header after everything else in the module (end of `module`); a free function's prototype says `inline` too (`free_fn`) | `dz_cpp/src/lib.rs` |
| An associated constant's function always in the header (`struct_bodies`, `MemberKind::Const`), so `Allocators::Global` inlines to its value | `dz_cpp/src/lib.rs` |
| `std`'s marked functions | `text/string.dz` (`len`, `is_empty`, `as_str`, `push`, `push_str`, `clear`), `core/hash.dz` (`hash_combine`), `collections/str_keys.dz` (`string_is`), `text/text.dz` and `text/chars.dz` (the iterators' `valid`, `get`, `advance`; `SplitWhitespace::find_word`), `core/formatter.dz` (`write_char`) |
| Tests | `tests/run/inline` (every kind of function with a body, in `WARNING_CLEAN`), `an_inline_body_is_in_the_header` in `dz/tests/run.rs`, `tests/errors/attribute_inline_without_a_body.dz`, `attribute_inline_on_a_struct.dz` |

**Done 2026-11-03** (r106), with link-time optimisation (T-0154). Measured against the
compiler before both, regions on, fastest of twenty alternating runs on a Ryzen 7 H 255,
clang 19 and g++ 14:

| Program | clang++, after / before | g++, after / before |
|---|---:|---:|
| astar | 0.826 | 0.957 |
| words | 0.823 | 0.871 |
| expr | 0.920 | 0.950 |

Taken apart by compiling the new C++ with and without `-flto`: on g++ `#[inline]` gives
nearly all of it (`words` 262 to 236 ms) and LTO adds about 5% on `astar`; on clang++
`#[inline]` alone changes little and LTO gives the rest. A debug build (`-O0`) inlines
nothing either way.

Left: a move still resets its source even when the source is never read again (T-0097),
and a sort still swaps by three moves (T-0096); both now cost less, since the reset's
`Allocators::Global()` inlines to two stores.
