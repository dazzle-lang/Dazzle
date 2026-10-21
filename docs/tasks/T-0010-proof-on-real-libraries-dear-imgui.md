---
id: T-0010
title: Proof on real libraries: Dear ImGui and Jolt fixtures
milestone: M7b
status: open
kind: feature
area: interop
---

**What is missing.** A `tests/run` fixture for **Dear ImGui** (over raylib or GLFW) and one
for **Jolt**, each built from vendored source the way raylib already is, each a real program
that compiles, runs and is checked against an `expected.txt`.

**Why these two, specifically.** raylib proves the C case, and it proves it well — it is a C
API of plain functions and plain structs, which is the easy half of §15. ImGui and Jolt were
chosen because between them they exercise **every hard thing** in C++ interop, and because
each is a library a real game would actually reach for:

| | ImGui | Jolt |
|---|---|---|
| Overloaded free functions | `Checkbox`, `Combo`, `TreeNode`, `BeginChild` ([T-0016](T-0016-overloaded-free-function-imports-only-its.md)) | some |
| Variadic functions | `Text`, `TextColored`, `LabelText` ([T-0008](T-0008-variadic-c-functions.md)) | — |
| Class hierarchies | backends | shapes, bodies, constraints ([T-0018](T-0018-inheritance-between-imported-c-classes.md)) |
| Abstract classes the user subclasses | render/platform backends | `ContactListener`, `BroadPhaseLayerInterface` ([T-0009](T-0009-dazzle-struct-implementing-imported-c-abstract.md)) |
| Templates | `ImVector<T>` | heavily |
| Namespaces | `ImGui::` | `JPH::` |

So these fixtures are not additional tests on top of the M7b features — they are **the
criterion for M7b**. Each of the rows above is a task that currently gets proven by a
hand-written fixture exercising one thing; a real library is what catches the interactions,
and the interactions are where interop goes wrong.

**What it would take.** The vendoring and build pattern is established — raylib is built from
source through `[cpp.libs]`, which is declarative with no build scripts
([T-0077](T-0077-git-dependencies-pinned-by-lockfile.md) wants to keep it that way) — so the
mechanics are a known quantity. The work is:

1. Vendor each, pinned, with the `[cpp.libs]` recipe to build it.
2. A program per library that uses the hard parts rather than the easy ones: for ImGui, a
   window with a checkbox, a combo and formatted text; for Jolt, a world with a falling body
   and a `ContactListener` of Dazzle's own that records the hit.
3. **Make them checkable without a screen.** ImGui and Jolt both normally need a window, and
   CI has none. Jolt is headless by nature — step the simulation N times and print the body's
   position. ImGui needs its null/dummy backend, or `--frames N`
   ([T-0065](T-0065-checking-game-without-looking-at-it.md)) plus a check on what the draw
   lists contain rather than on pixels.
4. Keep them **off the default test path** if they are slow to build, as `every_example_builds`
   presumably already handles for raylib — a multi-minute Jolt build on every `cargo test`
   would be its own problem.

Until these exist, M7b's features are each proven only against the fixture written for them,
and `docs/testing.md` should say so.
