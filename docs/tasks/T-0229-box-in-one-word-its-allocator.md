---
id: T-0229
title: "`Box` in one word: its allocator and drop beside the value"
milestone: none
status: done
kind: design
area: stdlib
where: `crates/dz_project/src/std/memory/box.dz` (`Box`, `BoxHeader`); `crates/dz_cpp/src/lib.rs`: `Box`'s upcast constructor (`handle_upcast_ctor`) and each polymorphic struct's `_dz_size` and `_dz_align`; `crates/dz_flow/src/release.rs`: `dyn_only_frees`; `crates/dz_check/src/check.rs`: `refuse_box_upcast_not_virtual` (DZ0588)
---

**A design question for the user**, raised 2026-11-14 after T-0228 made a `^mut Allocator`
one word. **Decided and done the same day (D-0104, r133):** the allocator in the
allocation, the drop and size of a derived value through the vtable, and a `Box` converting
only to a base with a `virtual` method (`DZ0588`).

**What it cost.** A `Box<T>` was three words, 24 bytes, against `std::unique_ptr`'s 8: the
pointer to the value, the allocator it came from (`m_allocator`), and `m_drop`, a function
made for the type the box was built as, so that a `Box<UiElement>` holding a `Button` drops
and frees a whole `Button` (D-0064, T-0097). A tree is mostly boxes: in the `expr`
benchmark, `Expr::Binary` holds two, and `Expr` was 64 bytes (`sizeof` in the generated
C++), where its C++ half's node holds two `unique_ptr`s.

**The options put to the user**, each with what it costs beside a full box's value:

- **The allocator in the allocation, the drop through the vtable** (chosen): 16 bytes, and a
  `Box` converts only to a base with a vtable.
- **The allocator and the drop function in the allocation**, as `Rc`'s block: 24 bytes,
  every conversion kept; only a null box saves.
- **The drop function only where something derives from `T`**, known program-wide: 16 bytes
  for most, but a layout that depends on every module.
- **Three words, unchanged.**

The allocator itself could go only where the compiler knows where the box was placed, which
it does not in general: a box lent to a function is dropped by code that never saw its
placement (spec §14.6).

**What was built.** `Box<T>` is `m_ptr` alone. Its constructor allocates a `BoxHeader`, the
allocator, then the value at the value's alignment, the header ending where the value
starts, so the drop finds it from `m_ptr`. The drop reads the size and alignment of what it
frees with `dyn_sizeof` and `dyn_alignof`, builtins only `std` can name: `T`'s own, or, for
a struct with a `virtual` method, through two hidden vtable entries every polymorphic struct
now has, `_dz_size` and `_dz_align`. `destroy` through the pointer is then the virtual
destructor. `Box<T>::size` is 8, and `expr`'s `Expr` 32.

**Found on the way, fixed here at the user's word:** an `Rc<Plain>` or `Box<Plain>` of a
`Fancy` that adds the first `virtual` below a `Plain` with none crashed when dropped
(`free(): invalid pointer`), since in C++ `Plain` sits a word into a `Fancy`, after its
vtable, and the drop was handed `Plain`'s address. `Rc`'s drop now finds the value from its
block; the `Box` conversion is the one `DZ0588` refuses. And a struct that added the first
`virtual` below a base with none had no virtual destructor when it had no `drop` of its own,
so a value deriving from it was destroyed as it; the first polymorphic struct of a chain now
declares one.

## Where the code is

| What | Where |
|---|---|
| `Box`, `BoxHeader`, its drop | `crates/dz_project/src/std/memory/box.dz` |
| `Rc`'s drop given the block | `crates/dz_project/src/std/memory/rc.dz`, `rc_block.dz` (`drop_value_as`) |
| `dyn_sizeof`, `dyn_alignof`: `std`-only builtins | `crates/dz_resolve/src/lib.rs` (`STD_BUILTIN_FNS`), `walk.rs`; `crates/dz_check/src/check.rs` (`builtin_arity`) |
| Their C++, `::dz::rt::dyn_size` / `dyn_align` | `crates/dz_cpp/src/runtime.rs`; `crates/dz_cpp/src/lib.rs` (`builtin`) |
| `_dz_size`, `_dz_align`, the virtual destructor | `crates/dz_cpp/src/lib.rs`, `struct_decl` |
| `Box`'s converting constructor | `crates/dz_cpp/src/lib.rs`, `handle_upcast_ctor` |
| `DZ0588` | `crates/dz_check/src/check.rs`, `refuse_box_upcast_not_virtual`; `crates/dz_diag/explain/check.md` |
| A box's drop only frees (T-0218) | `crates/dz_flow/src/release.rs`, `dyn_only_frees` |
