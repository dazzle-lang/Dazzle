---
id: T-0159
title: "Delete Allocators::Global, Allocators::Temp and TempArena: nothing outside std names an allocator"
milestone: none
status: open
kind: feature
area: stdlib
---

Decided by the user on 2026-11-03: the allocators will eventually be deleted. r104
(T-0119) took them out of ordinary code, since the compiler places memory (spec §14.6,
D-0091), but kept `Allocator`, `Allocators::Global` and `Allocators::Temp` for raw memory.
So a program can still write `Allocators::Global.alloc(…)`, outside `unsafe` even, which
the site's playground and ownership tour did until 2026-11-03. Deleting them leaves the
compiler's placement as the only way memory is chosen; `own` stays for what a C library
hands over.

A language change when it lands: a §21 row, §14 and §14.2 rewritten (15 mentions of the
two in the spec), and the decisions it revises (D-0047, D-0091's "kept for raw memory").

**Where the code is** (as of 2026-11-03)

| Piece | Where |
|---|---|
| The two names and their statics | `dz_project/src/std/memory/allocators.dz` |
| `TempArena` | `dz_project/src/std/memory/temp_arena.dz` |
| What `std` itself takes memory from, which must stay reachable inside `std`: the heap behind `Global`, the region arena, `Rc`'s block, memory tracking, a container's default `m_allocator` | `heap.dz`, `c_heap.dz`, `scope_arena.dz`, `scope_pool.dz`, `rc.dz`, `tracker.dz` in `std/memory/`; `List` and `HashMap` in `std/collections/` |
| Programs that name them | `tests/run/allocators` (8 uses), `elements_guard`, `tie_guard`, `iterator`, `generics`, `dynamic`; `tests/workspace/typed` and `typeerrors`; the lexer, parser and formatter corpora; `tests/errors/alloc_is_not_a_builtin.dz`, `std_libc_is_not_reachable.dz`; a comment in `tests/bench_programs/astar` |

**Open, for the user:** whether the `Allocator` trait itself stays public, for a program's
own container that takes `#placement` (r104), or becomes `std`'s alone; and what a program
that wants raw memory of its own does then (import `stdlib.h` with a recipe, as
`examples/ownership` and spec §14 describe).
