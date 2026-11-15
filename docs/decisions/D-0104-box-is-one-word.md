---
id: D-0104
title: "A `Box` is one word, and converts only to a base with a vtable"
status: decided
---

**A `Box<T>` is one word, the address of its value. The allocator the value came from is
kept in the allocation, in the word just before the value, and the size, alignment and drop
of what the box frees are those of the type it holds, or, when that type has a `virtual`
method, those of the type the value was made as, read through its vtable. So a
`Box<Derived>` converts to a `Box<Base>` only when `Base` has a `virtual` method, its own
or a base's (`DZ0588`). `Rc` and `Weak` convert to any base, as before (D-0064). Chosen by
the user on 2026-11-14 (T-0229); spec §14.5. Amends D-0064 for `Box`.**

**Why.** A tree is mostly boxes, and a `Box` was three words, 24 bytes against
`std::unique_ptr`'s 8: the value's address, its allocator and a function made for the type
it was built as, so that a `Box<UiElement>` holding a `Button` dropped and freed a whole
`Button`. That function was needed only for a box converted to a base, and a base with a
`virtual` method already has a table naming the type a value was made as. As one word, the
`expr` benchmark's `Expr` node is 32 bytes where it was 64; `expr` ran 2% slower on
clang++ and 5% faster on g++, with its peak memory unchanged, since each tree is dropped
before the next is built.

**What it costs.** Each allocation is a word larger, the allocator, rounded up to the
value's alignment, where each box is two words smaller, so a full box costs 16 bytes beside
its value where it cost 24, and a null one 8. Every struct with a `virtual` method has two
more entries in its vtable, its size and alignment, and dropping a box of one makes two
calls through it. A box converted to a base with no `virtual` method, which the old `Box`
allowed, is refused: C++ leaves the same deletion undefined for a `unique_ptr`.

**The options weighed** (T-0229):

- **The allocator in the allocation, the drop through the vtable** — this one.
- **The allocator and the drop function in the allocation**, as an `Rc`'s block keeps them:
  every conversion still allowed, but a full box costs as much as before, 24 bytes, and only
  a null one saves.
- **The drop function in the allocation only where something derives from `T`**, found
  across the whole program: 16 bytes for most boxes, but a layout that depends on every
  module, where everything else about a type is decided by its own.
- **Three words, unchanged.**

**Found on the way.** An `Rc<Base>` or a `Box<Base>` made from a struct that adds the first
`virtual` method below a base with none crashed when dropped: in C++ such a base sits a word
into the derived value, after its vtable, and the drop was handed the base's address. An
`Rc` now finds its value from its block; such a `Box` conversion is the one `DZ0588` refuses.
