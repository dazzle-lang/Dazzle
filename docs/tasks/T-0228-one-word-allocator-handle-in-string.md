---
id: T-0228
title: A one-word allocator handle in String, List and the other containers
milestone: none
status: open
kind: design
area: stdlib
where: `crates/dz_project/src/std/memory/` (`Allocator`, `Heap`, `ScopeArena`, `Allocators`); `std/text/string.dz`, `std/collections/*.dz` (their `m_allocator`); `crates/dz_cpp/src/lib.rs`: trait objects (`dz_vtable`, `Allocator__dyn`)
---

**A design question for the user**, raised 2026-11-14 from the `json` benchmark (T-0173,
T-0225).

**What it costs now.** Every container keeps the allocator it was placed with as a
`^mut Allocator`, a trait object of two words: the allocator's address and its table of
functions. So `String` is 40 bytes against `std::string`'s 32, and `List` 40 against
`std::vector`'s 24. In `json`'s tree that makes `Json` 48 bytes against 40 and a member 88
against 72, and the tree 237 MiB against 183. Touching the extra pages is about 20 ms of
the run (the kernel's `clear_page_erms` and page faults in a profile), most of what is left
between it and its C++ half after T-0225.

**Two ways to one word**, keeping the allocator (the user's point: removing it is not on
the table):

- **A thin pointer.** Each allocator keeps its table of functions as its first field, as a
  C++ object with virtual functions does, and a container holds only the allocator's
  address. `List` and `String` become 32 bytes. A `^mut Allocator` stays two words wherever
  else it is used; only the containers' field changes, to a new pointer type `std` would
  have to say how to build.
- **An index** into a table of the program's allocators, which can be 4 bytes. Alignment
  rounds a `List` to 32 bytes anyway, so it saves no more than the thin pointer unless
  another 4-byte field could share its word, and it adds a table lookup to every allocation
  and a limit on how many allocators exist, which regions, made and released all the time,
  would run into.

The thin pointer looks like the better of the two. Either changes every container, the
regions' allocator and the C++ the trait-object machinery emits, so it wants its own
measurement on every benchmark, not only `json`'s.
