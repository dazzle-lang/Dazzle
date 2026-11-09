---
id: T-0171
title: Dear ImGui controls in the SDL3 scene
milestone: M7b
status: done
done: r118, 2026-11-09
kind: feature
area: interop
---

Planned for 0.2 by the user on 2026-11-09: the C++ interop showcase. It is the ImGui half of
[T-0010](T-0010-proof-on-real-libraries-dear-imgui.md), as a real example rather than a
fixture; the Jolt half is not in 0.2.

**Done** (r118, 2026-11-09). The compiler side is built, and so is the panel, with a `Sky`
uniform added to the shaders so the night preset is dark. The example builds on clang++
and g++ and runs in a window at 60 fps. With the mouse: a checkbox and the scene's Space
key both change `paused` and each shows the other's change; dragging a slider leaves the
camera where it is; dragging the scene still turns it. The panel is scaled by the
display's content scale (and 1.25), and `screenshot.avif` now shows it.

| Piece | Where |
|---|---|
| ImGui, pinned to v1.93.0 | `examples/vendor/imgui` (submodule) |
| Its recipe, with `uses = ["sdl3"]` | `examples/sdl3/dazzle.toml` |
| The panel | `examples/sdl3/src/ui.dz`; what it changes is `Settings` in `src/scene.dz` |
| Default arguments | `Param::has_default`, `Walker::parameters` in `dz_cppimport`; read in `collect_foreign` and `foreign_methods`, `dz_check/src/lib.rs` |
| Array parameters | `Walker::array_parameter` in `dz_cppimport/src/clang.rs`; `dz::Array`'s explicit `T*` in `dz_cpp/src/runtime.rs` |
| Overloaded free functions | [T-0016](T-0016-overloaded-free-function-imports-only-its.md) |
| One type across imports | `unify_foreign_types` in `dz_resolve/src/lib.rs`; tested by `tests/run/shared_types` |
| `uses` | `CppLib::uses` in `dz_project/src/lib.rs`, read by `build_libraries` in `dz/src/native.rs`; tested by `uses_names_another_recipe_of_the_manifest` in `dz_driver/tests/cmake.rs` |

Variadics ([T-0008](T-0008-variadic-c-functions.md)) were not needed: the panel writes its
text with `format!` and `TextUnformatted`, so `ImGui::Text` waits for the user's decision
there.

**What it is.** `examples/sdl3` gets a Dear ImGui panel that changes the scene while it runs.
ImGui is built from its own source by a `[cpp.libs]` recipe, with its SDL3 and OpenGL 3
backends (`imgui_impl_sdl3.cpp`, `imgui_impl_opengl3.cpp`), and called directly from
Dazzle, with no wrapper:

```dz
imgui::ImGui::Begin("Scene")
imgui::ImGui::Checkbox("Paused", &mut paused)
imgui::ImGui::SliderFloat("Time speed", &mut speed, 0.0, 4.0)
imgui::ImGui::ColorEdit3("Sun", &mut sun_color)
imgui::ImGui::Text("%.0f fps", fps)
imgui::ImGui::End()
```

Controls worth having, from what `scene::Frame` and `main` already hold: pause and time
speed, the sun's angle and colour, each lamp's colour and orbit radius, fog density, the
camera reset, a preset `Combo`, an fps readout, and a `CollapsingHeader` per group. The
camera must not turn while a slider is dragged: `main` skips mouse events when
`ImGui::GetIO().WantCaptureMouse` is set.

**Why it matters.** raylib, SDL3, Box2D and sokol are all C. Nothing in `examples/` shows
that Dazzle calls a real C++ library, which is half of spec §15's promise, and ImGui is what
a game or tool actually reaches for to tune values at run time. A panel over a scene that
already looks good is also the best screenshot the project can have.

**What it is likely to need**, each its own task, done as the example reaches it:

| ImGui uses | Task |
|---|---|
| overloaded free functions: `Combo`, `TreeNode`, `BeginChild`, `Checkbox` with flags | [T-0016](T-0016-overloaded-free-function-imports-only-its.md) |
| printf-style variadics: `Text`, `TextColored`, `LabelText` | [T-0008](T-0008-variadic-c-functions.md); `TextUnformatted` with `format!` works without it, but the point is the interop |
| flag enums, `ImGuiWindowFlags_` combined with `\|` | [T-0048](T-0048-imported-c-or-c-enum-variants.md) |
| `float[3]` and `float[4]` parameters, from a `Vec3` | check how `&mut Vec3` reaches a `float*` today |
| default arguments (`Begin(name, p_open = NULL, flags = 0)`) | check whether the import fills them in |
| `ImGuiIO&` returned by reference, read by field | references are supported (r28); check |

**What it would take.**

1. Vendor ImGui as a submodule in `examples/vendor/imgui`, pinned to a release tag.
2. A recipe in `examples/sdl3/dazzle.toml`: the four core `.cpp` files and the two backend
   files. The SDL3 backend includes SDL3's headers, which the `sdl3` recipe installs into
   `build/cmake/`; find out whether one recipe can use another's installed include
   directory, and if not, that is a small manifest feature to add (compiler work).
3. The panel in a new `src/ui.dz`, kept apart so the scene code stays readable; `main` calls
   the backends' `NewFrame`, `Render` and `RenderDrawData` around it.
4. `every_example_builds` builds it; a new screenshot and a section in the example's README
   on what crossed the C++ boundary.
5. Each gap found gets fixed in its own task (table above), or recorded as one. Compiler
   work, no language change, unless a gap turns out to need one.
