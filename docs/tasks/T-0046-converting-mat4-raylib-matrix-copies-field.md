---
id: T-0046
title: Converting `Mat4` to raylib's `Matrix` is a transpose, not a cast
milestone: M12
status: open
kind: deferral
area: interop
where: a game's raylib binding code, not `std`, which knows nothing of raylib
---

**What goes wrong.** Handing a `std::Mat4` to raylib (`DrawMesh`, `rlPushMatrix`, a shader
uniform) means converting it to raylib's `Matrix`, and that conversion moves all sixteen
floats, once or twice per object per frame.

**Why no cast can make it free.** This task first assumed the two types hold the same
sixteen floats in the same order, so that an `unsafe` pointer reinterpretation (§10,
`p as ^T`) behind a size assertion would do. They do not. Both use column-major
*numbering*, but they store the elements differently:

- `Mat4` is `cols: [4]Vec4`, so memory holds column 0, then column 1, and so on, which is
  what OpenGL, GLSL, glm and HLSL's default `column_major` packing read.
- raylib's `Matrix` (`raylib.h`) declares `m0, m4, m8, m12` first, which is row 0, so its
  memory holds the matrix **row by row**. raylib transposes it back on every upload:
  `rlSetUniformMatrix` calls `rlMatrixToFloat` before `glUniformMatrix4fv`.

So the two are the same matrix stored in different orders: the bytes of one are the bytes
of the other's transpose. A cast would compile, pass any size and alignment check (both are
64 bytes, aligned to 4), and draw every object with a transposed transform. Converting is
sixteen copies **by name**, `m0 = cols[0].x`, `m1 = cols[0].y`, … `m4 = cols[1].x`,
since `mN` is element `N` counted down the columns. That is real work, which the C++
compiler turns into a few shuffles, but not free.

**What it would take.** Nothing in the language or in `std`, which knows nothing of raylib.

- **Write the conversion once**, in the code that binds raylib: the raylib example, or a
  raylib wrapper package once one exists. With reflection over fields (§13), it could be
  generated from the names, `mN` ← element `N`, which removes the risk of a slip in one of
  the sixteen lines.
- **Or don't convert**: a game that draws only through raylib can hold `Matrix` throughout
  and use raymath's functions on it.

Its cost beside the draw call it feeds is small, so this stays a deferral. Struct layout is
now a rule (D-0085, §7.3), so where another library's matrix really does share `Mat4`'s
layout, a pointer cast to it is sound.
