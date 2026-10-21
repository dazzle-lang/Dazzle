---
id: T-0071
title: Size and alignment on hover
milestone: M9
status: open
kind: feature
area: editor
---

**Size and alignment on hover** is *not* a small LSP feature, because **Dazzle has no
layout model**: `T::size` and `T::align` are emitted as C++'s `sizeof` and `alignof` and
the answer is the C++ compiler's. That is deliberate — it is what makes a Dazzle struct
ABI-identical to the C++ it is compiled to, and what lets an imported C++ class keep its
header's layout. So the editor cannot just look it up. Three ways, in preference order:
**ask libclang** (already a dependency, already loaded in-process, `clang_Type_getSizeOf`
and `getOffsetOf` over the generated header — accurate by construction and gives field
offsets, which is the genuinely useful part for a game programmer laying out data);
compile a probe translation unit and read it back (accurate, slower, another process); or
reimplement the platform ABI in Dazzle (fast, and wrong the first time the ABI disagrees).
The first is the one to build.
