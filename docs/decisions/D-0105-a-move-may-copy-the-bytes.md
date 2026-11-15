---
id: D-0105
title: "A move may copy the bytes: `T::is_relocatable`, and `List` moving its elements as bytes"
status: decided
---

**A value of any type Dazzle defines may be moved by copying its bytes into raw memory and
forgetting the source, which is then never dropped or read. `T::is_relocatable` asks it
(§5.5): yes for every type but a type parameter, an imported C++ class with a destructor,
whose move constructor is C++ code, and an opaque C handle. `List` grows, shifts and appends
relocatable elements with one `realloc`, `memmove` or `memcpy`, and starts with room for 2
elements up to a KiB, where it started with 4. Chosen by the user on 2026-11-14 ("we can try
it and see if there are issues with it"; then "2: less memory"), from T-0225's measurements;
spec §5.5, §14.3. Revises D-0081, which allowed a byte copy only for a trivially copyable
type, and moves the line T-0133 drew for D-0094's audit.**

**Why.** A Dazzle move is its fields' moves and a reset of the source to its defaults, and a
type has no move of its own to write (§6.6), so copying the bytes and never touching the
source again is the same move without the reset, which nothing reads when the source's memory
is about to be freed or overwritten. `List` moved every element that was not trivially
copyable one at a time: a `Json` node through a switch on its tag that resets the source,
and then a drop of that reset husk. In the `json` benchmark that cost 10 ms of g++'s 258, and
it made a smaller first allocation too dear: `json`'s short lists, a third of its records
holding a two-member object, each 0 to 4 tags, held about 14 MB the C++ half does not.

**What it costs.** C++17 calls such a copy undefined: the object's lifetime never starts in
the new memory. Every supported compiler compiles it as the move it stands for, as
relocating libraries (Qt, folly) rely on, and `-fno-strict-aliasing` already covers reading
the copied bytes. It is the one place the generated C++ steps outside C++'s object model on
purpose, recorded in `docs/testing.md`'s audit. A program can see one change: a moved-from
husk's `drop` no longer runs when a `List` grows or shifts, since there is no husk. A type
that keeps its own address in a field was never kept right by a Dazzle move either.

**Measured** on `json`, back to back: 239 ms and 209 MiB on clang++, 258 ms on g++, before;
with the byte moves and a first allocation of 2, 240 ms and 196 MiB, and 256 ms. With the
byte moves and the old 4, 236 and 248 ms and the old memory: the user chose the memory.
The other benchmarks did not move.

**The same evening**, at the user's word to proceed: `Deque` grows by copying its ring as
two runs of bytes, and `HashMap` moves each key and value into its new table as bytes, for
relocatable elements. Neither moved a benchmark; `tests/run/relocate` holds both.

**The options weighed** (T-0225):

- **Byte moves, and a first allocation of 2** — this one.
- **Byte moves, keeping 4**: 10 ms faster on g++, no less memory.
- **A first allocation of 2 with element moves**, inside D-0081: 13 MiB less, 9 to 13 ms
  slower.
- **Reusing a region's freed small blocks**: measured on every benchmark, it changed nothing,
  since none grows small containers inside a region; not kept.
