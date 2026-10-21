---
id: T-0019
title: A whole C array member cannot be copied or passed as a slice
milestone: M7b
status: open
kind: deferral
area: checker
where: `dz_check/src/check.rs`, C array member access
---

**What works and what does not.** An imported C struct with an array member — `float
values[16]`, `char name[32]` — imports fine, and everything you do *through* the array
works: indexing, `for`, `.len` and `.ptr` are all available.

```dz
for v in mat.values { … }               // fine
let x = mat.values[3]                    // fine
let n = mat.values.len                   // fine
```

What is refused (`DZ0707`) is treating the array **as a whole value**:

```dz
let copy = mat.values                    // DZ0707: cannot copy a C array member
take_slice(mat.values)                   // and it is not a []f32
```

**Why.** A C array member is neither of the two things Dazzle has. It is not a Dazzle
`[N]T` — that is `dz::Array<T, N>`, a struct with its own copy semantics — and it is not a
slice, which is a `{ptr, len}` pair. The member is raw storage inside the header's struct,
and C's own answer (an array decays to a pointer, and assigning one is not allowed either) is
not a semantics Dazzle wants to inherit.

**Why it matters.** Chiefly for the two shapes that come up constantly in C APIs: a fixed
matrix or vector member you want to hand to a function taking a slice, and a fixed character
buffer you want to read as text. The second has a workaround already — build a `str` from
`.ptr` and a length — and the first does not, so a `[]f32` parameter cannot be fed from a C
struct's array at all.

**What it would take.** Two independent pieces, and they are both small:

1. **The slice.** `mat.values` in a `[]T` position builds a `Slice{ptr, len}` at the use
   site, from the member's address and its known length. The length is in the imported type,
   so nothing has to be discovered. This is the one worth doing — it is a few lines in the
   checker and the emitter, and it is what unblocks real code.
2. **The copy.** Copying needs a helper that fills a `dz::Array<T, N>` element by element
   (or one `memmove`, if [T-0042](T-0042-list-insert-remove-move-elements-one.md)'s
   relocation primitive lands first), and a decision about what the result's *type* is — a
   Dazzle `[N]T`, presumably, which then has Dazzle copy semantics rather than the header's.
   Lower value than the slice.

`DZ0707`'s message should name both and say which is owed, so the error teaches the
workaround.
