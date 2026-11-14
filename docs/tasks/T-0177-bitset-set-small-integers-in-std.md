---
id: T-0177
title: BitSet: a set of small integers in std
milestone: M12
status: done
kind: feature
area: stdlib
---

**Done** (2026-11-14, r127). The counts are `Integer` methods, `count_ones`,
`trailing_zeros` and `leading_zeros`, which the emitter turns into `::dz::rt::num`'s of
the same name, as it does every `Integer` method; the shim's are `__builtin_popcountll`,
`__builtin_ctzll` and `__builtin_clzll` on the unsigned type of the same width, with zero
answered first, where the builtins are undefined. Beyond the plan: `==` by the integers
held, `is_empty`, `capacity` and `reserve`. Writing `insert` found T-0213: `let was =
m_words[word]` bound a reference to the word, not a copy.

## Where the code is

| What | Where |
|---|---|
| the set | `crates/dz_project/src/std/collections/bit_set.dz` |
| `for n in set` | `crates/dz_project/src/std/collections/bit_set_iter.dz` |
| the counts, declared | `crates/dz_project/src/std/math/integer.dz` |
| the counts, in C++ | `crates/dz_cpp/src/runtime.rs`, `namespace num`, after `is_power_of_two` |
| an `Integer` method's C++ | `crates/dz_cpp/src/lib.rs`, the `"Float" \| "Integer" \| "Number"` arm |
| test | `tests/run/bit_set` |

Planned for 0.2 by the user on 2026-11-09.

**What it is.** A growable `BitSet` in `std::collections`, one bit per integer, stored in a
`List<u64>`: `insert`, `remove`, `contains`, `len` (a population count), `clear`, iteration
over the set integers in order, and `union_with`, `intersect_with` and `difference_with`.

```dz
let mut visited = BitSet()
visited.insert(cell.index())
if !visited.contains(next.index()) { ... }
```

**Why it matters.** A `HashSet<usize>` or a `List<bool>` does the same job with 8 to 64
times the memory. Visited sets in grid searches, collision layers and masks, and which
components an entity has (the ECS benchmark,
[T-0172](T-0172-ecs-benchmark-against-rust-c-query.md)) all want one.

**What it would take.** `std/collections/bit_set.dz`, listed in `STD_SOURCES`
(`crates/dz_project/src/lib.rs`). Its elements are integers handed out by value, so it needs
no `tie [*]`; iteration ties to `@items`. Population count and trailing zeros come from the
C++ compiler's builtins through the runtime shim, on both clang++ and g++ (invariant 7), or
from a plain loop if the shim has none yet. A `tests/run/bit_set` program.
