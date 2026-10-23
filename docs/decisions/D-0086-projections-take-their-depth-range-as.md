---
id: D-0086
title: Projections take their depth range as a required argument
status: decided
---

**`Mat4::perspective` and `Mat4::orthographic` take a `DepthRange`, `.MinusOneToOne` or
`.ZeroToOne`, as their last argument, with no default. Chosen by the user on 2026-10-23.**

`Mat4`'s memory layout suits every graphics API Dazzle is likely to draw with: OpenGL,
Vulkan, Direct3D, Metal and SDL3's GPU API all read it as it is (raylib's `Matrix` is the
exception, as a type of its own, T-0046). What differs between them is where a projection
puts depth. OpenGL maps `near`..`far` onto -1..1; the rest map it onto 0..1. Until r77 `std`
had only OpenGL's, and the wrong one does not fail: it clips geometry near the camera and
wastes depth precision.

- **Both, chosen at the call (chosen).** The choice is visible where the matrix is built,
  and a program that picks its API at run time passes a variable.
- **No default.** Defaulting to OpenGL's range would let a Vulkan program skip the
  argument and draw wrongly with nothing to say why.
- **Chosen per project, by import or by `dazzle.toml` (rejected).** The user's first idea.
  It would make one call, `Mat4::perspective(…)`, return different matrices in different
  projects, decided by a setting nowhere near it, which D-0057 rules out for names. It is
  glm's `GLM_FORCE_DEPTH_ZERO_TO_ONE`, a known trap when a library and a game are built
  with different settings, and it cannot serve an engine that chooses its API at run time.
  A program that does want one choice for its whole build gets it from its own code: a
  `const` of type `DepthRange`, or one picked by `#if` on a flag it registers (§12.10).
- **Separate names, `perspective_zero_to_one` (rejected)**: the choice can be skipped by
  calling the shorter name, and a run-time choice needs an `if` at every call.

Clip-space y is left alone: it points up everywhere but Vulkan, where the usual fix is a
viewport of negative height, not a different matrix.

Spec: §14.4.
