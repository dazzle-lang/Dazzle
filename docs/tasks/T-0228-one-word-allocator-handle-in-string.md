---
id: T-0228
title: A one-word allocator handle in String, List and the other containers
milestone: none
status: done
kind: design
area: stdlib
where: `crates/dz_cpp/src/runtime.rs` (`ThinDyn`); `crates/dz_cpp/src/lib.rs` (`thin_trait`, `thin_impl`, `struct_decl`'s hidden `_dz_vt`, `impl_vtables`, `trait_object_bodies`, statics after the vtables); `crates/dz_check/src/impls.rs` (`check_allocator_impls`, `DZ0587`)
---

**Done 2026-11-14: every `^mut Allocator` is one word** (D-0103, r132, the user's choice of
the three ways below). Nothing in `std`'s source changed: the containers still hold a
`^mut Allocator`, which is now the allocator's address alone.

| What | Where the code is |
|---|---|
| the one-word handle, reading the vtable from the object's first word | `crates/dz_cpp/src/runtime.rs`: `ThinDyn` |
| which trait is thin (`Allocator`), and which struct implements it | `crates/dz_cpp/src/lib.rs`: `thin_trait`, `thin_impl` |
| the hidden first member `_dz_vt`, and `dz_vtable` declared before the struct | `crates/dz_cpp/src/lib.rs`: `struct_decl` |
| the vtable as a `constexpr` variable, so `HEAP` and `TEMP` stay constant-initialised | `crates/dz_cpp/src/lib.rs`: `impl_vtables`; statics are emitted after it |
| the handle's methods, and the region's direct call (T-0151) | `crates/dz_cpp/src/lib.rs`: `trait_object_bodies` |
| only a plain struct of its own module implements `Allocator` | `crates/dz_check/src/impls.rs`: `check_allocator_impls`, `DZ0587` |

Measured on every benchmark: see `docs/log/tooling.md`, "A one-word allocator handle".

What follows is the question as it was asked.

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
