---
id: T-0225
title: "What keeps json behind its C++ half: float text, and a union moved through every !-> return"
milestone: M15
status: open
kind: feature
area: codegen
where: `tests/bench_programs/json/`; `crates/dz_cpp/src/lib.rs`: the lowering of `or return` on a `!->` call, `let` of its value and `push(move(v))`; `crates/dz_cpp/src/runtime.rs`: `Res`, `format_float`
---

What the `json` benchmark (T-0173) left, measured 2026-11-14 on clang++ with `perf` (IBS
precise sampling) and the phases timed apart. It was 1.40× its C++ half on clang++ and 1.56×
on g++, after three changes that came with the benchmark: `List`'s first
allocation sized by its element (376 → 329 ms, 316 → 237 MiB), `parse_f64` through
`std::from_chars` (parse 160 → 142 ms), and the checker no longer overflowing its stack on a
union holding a `List` of itself.

| Phase, ms | Dazzle | C++ |
|---|---:|---:|
| generate the text | 22 | 22 |
| parse it into a tree | 141 | 111 |
| the tree to `List<Record>` | 37 | 20 |
| write it out | 67 | 26 |

- **Writing: float text, about 30 ms of the 41.** *Done the same day: r130 writes floats in
  full, through `std::to_chars`, and `json` went to 1.25× and 1.42× (`RESULTS.md`).* It was: `{}` on a float is `snprintf("%g")`
  (glibc's `printf_fp`, `hack_digit`, `mpn_divrem`); the C++ half's `std::to_chars` hardly
  shows. Changing it changes what every program prints, so it is T-0223, the user's
  decision. The rest is the reflective writer's own work, a type-table lookup per value.
- **Parsing, 30 ms.** Each element a parse returns travels as a `Res<Json>`: the generated
  `array()` moves it out of the result into a local, from the local into `push`'s
  by-value parameter, and from there into the list's storage, each a switch on the tag
  that resets the source, and then destroys the emptied result and local
  (`Json::operator=(Json&&)` 567 samples of 50 000 cycles, `List<Member>::push` 1160,
  `move_to` 452; the C++ half's `push_back` is inlined). `push` marked `#[inline]` changed
  nothing, so it is the moves, not the call. Constructing in place from the result, as the
  C++ half's `push_back(std::move(v.value()))` does in one move, is the change to look at,
  with T-0097's ground: a union through a fallible result.
- **Converting, 17 ms.** Copying each string out of the tree and building each `Record` by
  struct literal; not broken down further.
- **Memory, 236 MiB against 180.** Dazzle's types are larger: `String` 40 bytes against 32,
  `List` 40 against 24 (its allocator is a two-word handle), so `Json` is 48 against 40 and
  `Member` 88 against 72; and a region does not reuse what a growing list leaves behind
  (with every value on the heap, `DZ_REGIONS=0`, it was 296 MiB against 316 before the
  capacity change, and no faster).

**Progress, 2026-11-14.** The parse's moves: `move(x)` of a copyable local is a plain C++
move where the reset cannot be seen, and `let v = f() or return` names the value in its
result, both where `drops_silently` (no `drop` of the program's own in the value). With
`cut_bytes`, `write_char` and `String`'s `drop` inline, `json` went from 1.25× to 1.16× on
clang++ and 1.42× to 1.31× on g++ (`RESULTS.md`). Now, in ms on clang++: parse 120 against
111, convert 36 against 20, write 36 against 25. The largest single cost left is memory, 234
MiB against 181 and about 20 ms of page faults, from the containers' two-word allocator
handle: T-0228, a design question. Convert's copies of every string out of the tree and
the writer's per-value type lookups are the rest.
