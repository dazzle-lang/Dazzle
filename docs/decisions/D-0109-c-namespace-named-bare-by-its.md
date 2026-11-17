---
id: D-0109
title: A C++ namespace is named bare by its dependency's namespaces key
status: decided
---

**A `cpp` dependency may list namespaces of its header to be named bare in every file of
the module: `imgui = { cpp = "imgui", namespaces = ["ImGui"] }` makes `ImGui::Begin` mean
`imgui::ImGui::Begin`, which still works. A nested namespace is named by its last segment,
`"b2::detail"` as `detail`. The name must be free in the module: a dependency key, a
declaration of the module, a `[names]` choice or another namespace named so is `DZ0318`,
never shadowed. Chosen by the user on 2026-11-17, in T-0179 ("on the .toml file, we can
indicate which namespaces to make global, which obviously the compiler has to watch for
collisions"), for accessibility and so that LLMs write C++ calls as C++ does; spec §3.2,
§15.2, r141.**

**Before.** A C++ namespace was a nested scope of its header's module, so every call into
Dear ImGui was written `imgui::ImGui::Begin`: the dependency key, then the namespace. Most
C++ libraries keep everything in one namespace (`ImGui::`, `b2::`, `JPH::`, `fmt::`), so the
doubled path was the first thing a C++ programmer saw in Dazzle code that called C++.

**The options T-0179 listed, not taken.**

1. A `namespace` key that makes the dependency key itself name the namespace,
   `ImGui = { cpp = "imgui", namespace = "ImGui" }`. One namespace per key, and the header's
   global declarations (ImGui's backends, `ImGui_ImplSDL3_Init`) would need a second key
   on the same recipe.
2. `via` and `import` reaching into an imported namespace, `ImGui = { via = "imgui::ImGui" }`.
   No new key, but a `via` names a module, and a namespace is not one: it would have blurred
   what `via` means for every dependency to serve one kind.

**Why this one.** The dependency keeps its key for the header's globals, `imgui::ImGui_ImplSDL3_Init`,
and the namespaces it lists read as C++ writes them, `ImGui::Begin`, so code copied from a
C++ example needs only its syntax changed. Listing several namespaces of one header, nested
ones included, costs nothing more. It reuses `[names]`' mechanism, a bare name chosen in the
manifest, so resolution, the checker and the editor see one rule. **A collision is an error
rather than a precedence**, unlike an opened dependency's export, which anything named
explicitly hides (§3.2): `ImGui::` meaning a struct of the module in one reading and the
namespace in another is the confusion the feature exists to remove.

**Left for later.** The full path stays valid. Pointing out `imgui::ImGui::x` where `ImGui`
is named bare, with a `dz fix` that drops the key, is T-0243, for 0.3 (the user, the same
day). A namespace named bare is the module's own; it is not passed on to modules that
depend on it, which reach the header through its key as before.
