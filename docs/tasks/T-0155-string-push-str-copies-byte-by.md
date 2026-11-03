---
id: T-0155
title: String::push_str copies byte by byte, and push encodes every character
milestone: M12
status: done
kind: feature
area: stdlib
---

A `gprof` profile of `tests/bench_programs` (2026-11-03, after T-0153 and T-0154) put
`String::push_str` at a quarter of `words`' time: 21.8 million calls, each pushing its
bytes one at a time through `List::push`, which checks the capacity for every byte. And
`String::push` encoded every character into a 4-byte buffer and handed it to `push_str`,
ASCII included, 14 million times.

**Where the code is**

| Piece | Where |
|---|---|
| `push_str`: reserve once, then `extend` | `String::push_str` in `dz_project/src/std/text/string.dz` |
| `push`: an ASCII character written where the closing zero was, then a new zero; anything else encoded as before | `String::push` in the same file |
| `extend` and `insert_slice` copy a trivially copyable element type with one `memcpy` | `impl Extend<T> for List<T>` in `dz_project/src/std/collections/extend.dz` |
| Test: ASCII pushed from empty, between other characters and after `push_str`, the closing zero checked through `as_cstr` | `tests/run/containers` |

**Done 2026-11-03.** Measured against the compiler before it, fastest of twenty
alternating runs: `words` 17% faster on clang++ and 9% on g++; `expr` unchanged. `astar`
measured about 5% slower on clang++ and unchanged on g++, though its generated C++ is
identical and it calls none of these functions in its loop: `main`, where clang inlines the
search, starts 16 bytes later because `std`'s code before it changed size, and a tight
loop's speed moves with its alignment.
