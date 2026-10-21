---
id: D-0038
title: How C's unscoped enums and `#define`s come in
status: decided
---

**An unscoped enumerator is also an untyped integer constant** at the header's scope, beside the enum itself: raylib's `IsKeyDown(int key)` takes `rl::KEY_A` as it is, and `.KEY_A` still names the typed variant wherever the enum is wanted. An `enum class` keeps its enumerators behind its name. **An object-like macro is a constant when its value can be typed**, emitted by its own name: at M7a a number (an integer untyped, a float `f32` with an `f` suffix and untyped without), with other expressions clang can type — `RAYWHITE`'s compound literal — to follow. Typed enumerators, which would need a cast at every raylib input call, were the alternative (§15.2)
