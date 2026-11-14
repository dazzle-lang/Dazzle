---
id: D-0103
title: "A `^mut Allocator` is one word: each allocator holds the table of its functions"
status: decided
---

**A `^mut Allocator` or `&mut Allocator` is one word, the allocator's address. Every type
that implements `Allocator` keeps the table of its functions as a hidden first field, as a
C++ class with `virtual` functions keeps its own, and only a struct declared in the
implementation's module, with no type parameters, no base and no `virtual` method, may
implement it (`DZ0587`). Every other trait's `^Trait` stays two words, and its implementing
types unchanged (D-0006). Chosen by the user on 2026-11-14 (T-0228); spec §7.6, §14.**

**Why.** Every container keeps the allocator it was placed in (§14.6), so the handle's size
is paid by every `String`, `List`, `HashMap`, `Deque` and `Box`, and by every function
returning one of them, which takes where its result goes as a hidden argument. As two words
it made `String` 40 bytes against `std::string`'s 32 and `List` 40 against `std::vector`'s
24; in the `json` benchmark's tree a member was 88 bytes against 72, and the tree 237 MiB
against 183, and touching the extra pages was most of what was left between it and its C++
half (T-0225). As one word, `String` and `List` are 32 bytes. An allocator is a few
long-lived objects, so a word more in each costs nothing, where a word more in each
container is paid millions of times.

**What it costs.** A call through a `^mut Allocator` loads the table from the allocator
before the function from the table: the allocator's first word, which the call is about to
touch anyway. A program's own allocator must be a plain struct, as `std`'s three (`Heap`,
`TempArena`, `ScopeArena`) are; what it would have been, an enum or a generic type, it holds
as a field. `size_of` an allocator is a word more than its fields.

**The options weighed** (T-0228):

- **Every `^mut Allocator` one word** — this one.
- **Only the containers' field one word**, a new pointer type `std` builds from a
  `^mut Allocator`, which stays two words elsewhere. It needs the same hidden field in every
  allocator and a new pointer type, and leaves the hidden result argument two words.
- **A trait opts in to thin objects**, an attribute any trait could carry, with `Allocator`
  the first. The same work and more: the attribute and its rules, for traits that have not
  asked for it.
- **An index into a table of the program's allocators**, 4 bytes. Alignment rounds a `List`
  to 32 bytes anyway, so it saves no more than a pointer, and it adds a lookup to every
  allocation and a limit on how many allocators exist, which regions, made and released all
  the time, would run into.
