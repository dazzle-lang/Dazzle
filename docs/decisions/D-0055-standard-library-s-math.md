---
id: D-0055
title: The standard library's math
status: decided
---

**A compiler-carried module `std::math`, loaded by `import std::math`; the name `std` reserved.** Chosen over the prelude, which would put `Vec3` and `PI` in every file, and over a plain `math`, which a project's own module could take. `f32` vectors, `Quat` and `Mat4`, plus `Vec2i`/`Vec3i` for grids — a generic `Vec3<T>` would wait on associated-type projection. **Methods everywhere**, scalars included through `Float` and `Integer` (`x.sqrt()`, `n.wrapping_add(m)`), since without overloading a free `lerp` could not serve `f32` and `Vec3` both. **Column-major matrices** on column vectors, as OpenGL, GLSL and glm, so a `Mat4` is `projection * view * model` and its floats are in the order a shader wants (§14.4). *Loading by `import std::math` superseded by 18.57:* the math is part of the one flat `std` module
