---
id: T-0225
title: "What keeps json behind its C++ half: float text, and a union moved through every !-> return"
milestone: M15
status: done
kind: feature
area: codegen
where: `tests/bench_programs/json/`; `crates/dz_project/src/std/collections/list.dz` (`grow_to`, `move_to`: the first allocation and byte moves, D-0105); `crates/dz_project/src/std/memory/scope_arena.dz` (region chunks); `crates/dz_cpp/src/lib.rs`: the lowering of `or return` on a `!->` call, `let` of its value and `push(move(v))`; `crates/dz_cpp/src/runtime.rs`: `Res`, `format_float`
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

**Progress, later on 2026-11-14.** T-0228 made a `^mut Allocator` one word (r132, D-0103):
`String` and `List` are 32 bytes, `Json` 40 and `Member` 72, as in the C++ half. `json` went
from 1.15× to 1.07× on clang++ and 1.32× to 1.24× on g++, and its peak memory from 236 MiB
to 208 against 181. The 27 MiB left is not the types' size; what a region keeps of a list
that grew is the suspect, not yet measured on its own.

**Progress, the evening of 2026-11-14: the memory, measured.** The 28 MiB left, taken
apart with `valgrind --tool=massif` on the Dazzle half with regions on and off
(`DZ_REGIONS=0`) and on a C++ half built with `-g -fno-inline` so its allocations are
named:

| `json`, g++ | Time | Peak |
|---|---:|---:|
| regions (the default) | 238 ms | 207.7 MiB |
| every value on the heap | 232 ms | 197.8 MiB |
| the C++ half | 210 ms | 179.7 MiB |

- **Most of it was how big a new `List` starts**: room for 4 elements, as Rust's `Vec`,
  where libstdc++'s `vector` starts at 1 and doubles. A third of the records hold a
  two-member `meta` object, each holds 0 to 4 tags and a third 1 to 3 `seen` numbers, so
  about 14 MB of short lists' room the C++ half does not have; the tags lists measured
  9.7 MiB against 6.7.
- **A first allocation of 2 cost time** (247 ms, 196 MiB on clang++), because a `List<Json>`
  that grows moved each element through a switch on its tag that resets the source. D-0105
  (r134, the user's choice) lets a move copy the bytes of any type Dazzle defines: with
  that, 2 costs nothing (240 ms, 196 MiB, against 239 and 209), and 4 would have been 10 ms
  faster on g++.
- **Not what the region keeps of an outgrown list**, the suspect before: a region that
  reused its freed small blocks by size class changed nothing on any benchmark, since no
  benchmark grows small lists inside a region (`json`'s tags lists never outgrow their
  first allocation), and was not kept. The 10 MiB regions add to `json` is in its chunks:
  the larger chunks are mostly empty while the program runs, not yet measured further.

**Then what regions add, measured again** (resident memory sampled every half millisecond,
regions on and off): 195.6 MiB against 185.6. The gap opens while the tree is turned into
records, and stays to the end, yet the live heap `massif` counts at that point is no
larger with regions (171 MiB against 178, at slightly different moments). What the evidence
points to: in the heap build the records' small tag lists are allocated into the holes
freeing the parse's temporaries left in `malloc`'s heap, and in the region build they go
into fresh region chunks, which touch new pages while those holes sit unused. It is what an
arena costs when it lives as long as the program, `main`'s outermost region here, and a
question about regions, not about `List`.

So the gap is now 196 MiB against 180 and, on g++, 256 ms against 211: about 10 MiB of
region chunks, the input and output text a few MiB larger than C++'s, and malloc's own
overhead on the heap build.

**Progress, 2026-11-15: not safety, and two causes fixed (T-0234).** With every panic made
unreachable in the generated C++ and the three flags that define overflow, aliasing and
endless loops reversed, `json` ran no faster on either compiler. Timed by phase, parsing was
already level on clang++ (107 ms against 108); the gap was in converting (29 against 20)
and writing (38 against 26), and on g++ in parsing too (105 against 93). T-0234 fixed a
pushed element's extra move and g++'s unit-growth limit, which had left `String::push_str` a
call: `json` is 1.06× on clang++ and 1.13× on g++ (`RESULTS.md`). What is left, each
measured by a patch to the generated C++:

- **The writer is reflective and the C++ half's is by hand: 10 ms on clang++, 13 on g++.**
  `write_value` looks the type up, tests `info.name.starts_with("List<")` and scans the
  fields for `m_data` and `m_len` for every value. A `record_json` written by hand in Dazzle
  writes in 27.7 ms against C++'s 25.7. Whether the benchmark is meant to price reflection
  is the user's call; if it is, the report should say so, and if not, the writer is by hand
  on both sides as the reader already is.
- **A float's text takes a round trip: 2.5 ms on clang++.** `shortest_digits` asks
  `std::to_chars` for the scientific form and reads the exponent back with `atoi` (44 M
  instructions in `strtol`), and `shortest_fixed` lays the digits out again. For a value
  between 1e-3 and 1e15 whose `std::to_chars` text has no exponent, that text is already
  what D-0101 asks for; taking it there gave the same checksum.
- **Converting, 7–8 ms on both**: about 4–5 in the bare loop, a 144-byte `Res<Record>`
  built per record and five destructors run on values already moved from (T-0231), and
  about 3 in the tags lists, which take 3,800 page faults against the C++ half's 2,000,
  with regions on or off, for a reason not found.
  Forcing `text_of` and `String`'s constructor and moves inline was worth 1 ms.
- **`main`'s region, 4 ms** (T-0233): with `DZ_REGIONS=0`, parsing is 4–7 ms faster and
  converting 2 slower.

With the first two and T-0234's fixes done by hand, `json` ran 0.99× on clang++ and 1.08×
on g++.

**2026-11-15, where it stands.** Measured on `e2fb6f3`, after values moved by default
(T-0235) and before T-0222, which did not move it: 224 ms against 222 for the C++ half on
clang++, 1.01×, and 222 against 209 on g++, 1.06×, at 185 MiB against 180. The run before
that, at `7875aab` with T-0234, had 234 against 221, 233 against 207, and 195 MiB. What
is listed above as left was not measured again.

**2026-11-16: the float text is done.** For a value from 1e-3 up to 2^24 for an `f32` and
1e15 for an `f64`, `std::to_chars`' own shortest text, when it has no exponent, is written as
it is: below those bounds every whole number is exact, so the fixed form never writes more
digits than the shortest ones. Above 2^24 an `f32`'s fixed text is its exact digits,
`737850294272` where D-0101 writes `737850300000`, which is why the bound is there. Held to
the general path on 226 million values, random bit patterns, decimals of few digits and
whole numbers, on both compilers: the same bytes. `tests/run/float_text` has both sides of
each bound, with and without `to_chars`. On `json` alone: 2.2% fewer instructions on
clang++ and 2.4% on g++ (`shortest_fixed` in `runtime.rs`).

**2026-11-16: the reflective writer works out each type once**, the user's choice over
writing `to_json` by hand or generating it with a macro (§13.4): the benchmark keeps pricing
reflection at run time, done as a reflective serializer is done well. The writer (`Writer`
in `tests/bench_programs/json/src/main.dz`) prepares a `Plan` per `TypeId` the first time
it meets the type: an object's public fields with their keys already quoted, `"name":`, and
their tags read; a list's storage and length offsets; an optional's. Writing a value reads
its plan, where it compared the type's name with `"List<"`, read every field's tag again
(`tag_value`, which also checks it is UTF-8 each time) and searched each list's fields for
`m_data` and `m_len`, for every value. The checksum is the same.

Against the build before it (instructions, and 11 interleaved runs):

| `json` | Instructions | Time | Against the C++ half (`RESULTS.md`) |
|---|---:|---:|---:|
| clang++ | -4.0% | -2.2% | 0.93×, from 0.96× |
| g++ | +3.3% | level | 1.06×, as it was |

The writer's own instructions halve on both compilers (g++: 337 M to 153 M). On g++ the
total still rises, because g++ then decides differently elsewhere: `String::push_str` is no
longer inlined into `Formatter::write_str` (`max-inline-insns-single`), since g++ first
inlined `make_room` into `push_str`; before, `make_room` stayed a call and `push_str` stayed
small enough. Marking `make_room` `cold, noinline` by hand helped g++ with this writer
(-1.5%) and hurt it with the old one (+2.1%), so it is inlining luck, not an attribute to
add. The writer is more code: `json`'s release build from clean is 0.96 to 1.01 s on
clang++ and 1.08 to 1.23 s on g++ (C++: 0.72 and 0.70).

**Measured again and closed, 2026-11-17** (at r140, `519eb50`, a quiet machine, load 0.33;
`RESULTS.md`, and the run recorded in `HISTORY.md`):

| `json` | Dazzle | C++ | Dazzle / C++ | Peak memory | Build from clean, release |
|---|---:|---:|---:|---:|---:|
| clang++ | 204 ms | 221 ms | 0.92× | 185.2 / 179.6 MiB | 1.05 / 0.71 s |
| g++ | 210 ms | 208 ms | 1.01× | 186.9 / 179.7 MiB | 1.11 / 0.70 s |

The g++ side's last step, from 1.05× to 1.00–1.01×, came with r137 (`#[inline]` inlines
every call, D-0107) and has held in every run since; the history (T-0075) shows it.
`json` is level with its C++ half on g++ and ahead on clang++, so nothing here keeps it
behind any more. What remains is not a gap in time:

- **Peak memory, 5–7 MiB above C++'s**, the region chunks of `main`'s outermost region
  (above, and T-0233); a question about regions, not about `json`.
- **The build, 1.5× C++'s from clean in release**: the reflective writer's plans and the
  instances, where the C++ half writes `to_json` by hand. The benchmark prices reflection on
  purpose (the user's choice, 2026-11-16).

Neither is a reason to keep this open. A regression in run time or build time now shows
as a ratio that moved in `tools/bench_programs.py`'s comparison; peak memory is recorded
in the history but not compared.
