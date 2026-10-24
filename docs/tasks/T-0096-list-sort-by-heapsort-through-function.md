---
id: T-0096
title: List::sort_by is a heapsort through a function pointer: five times slower than std::sort
milestone: M12
status: open
kind: feature
area: stdlib
where: `crates/dz_project/src/std/collections/list.dz`, `sort_by` and `sift_down`; `sort.dz`, `compare_bytes`
---

**What it is.** `xs.sort_by(before)` and `xs.sort()` are a heapsort, and the comparison is
a `fn(&T, &T) -> bool` value called through a pointer on every step:

```dz
entries.sort_by(before)          // before: fn(&Entry, &Entry) -> bool
```

The whole-program benchmark `tests/bench_programs/words` (T-0090) sorts 130,275
`{ String, i32 }` entries by count and then by word. On a Ryzen 7 H 255 with g++ 16 at
`-O2`, that one call takes about 100 ms. `std::sort` with a lambda takes about 19 ms on the
same entries. It is the largest single gap between that program's two halves: the rest of
the program is within about 1.3×.

Three things add up:

- **Heapsort.** It was chosen because it allocates nothing and has no bad inputs (the doc
  comment says so). But it jumps around memory and makes about twice the comparisons of
  quicksort. `std::sort` is introsort: quicksort, with heapsort as the fallback that gives
  the same `n log n` guarantee, and insertion sort for short runs.
- **The comparison is not inlined.** `before` reaches `sort_by` as a function pointer, and
  `sort_by` is not specialised on it. A C++ lambda is its own type, so `std::sort` is
  instantiated with the comparison inlined.
- **`String` order compares byte by byte** (`compare_bytes`), where `std::string` calls
  `memcmp`.

**Why it matters.** Sorting is in nearly every real program: a scoreboard, a draw order, a
list of files. And it is the number a reader of the published benchmark will notice.

**What it would take.** Library work only, no language change.

1. An introsort or a pattern-defeating quicksort (pdqsort) in `list.dz`. It still allocates
   nothing, and heapsort stays as its depth-limit fallback, so the documented guarantee
   holds. `sort()` on `Ord` elements gets it for free.
2. `compare_bytes` through `memcmp` on the shorter length (the runtime already calls C for
   memory), then the lengths.
3. Inlining the comparison needs a generic parameter for it, `sort_by<F>`. Whether
   Dazzle's generics can take a function as a type the way C++ takes a lambda is a question
   for the user: it is closer to a language change. Until then, a direct call through a
   known function pointer is often inlined by the C++ compiler once `sort_by` itself is
   small enough to inline; measure before deciding.

Re-run `python tools/bench_programs.py words` before and after.
