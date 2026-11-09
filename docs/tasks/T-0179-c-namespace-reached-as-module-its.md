---
id: T-0179
title: A C++ namespace reached as a module of its own: ImGui::Begin, not imgui::ImGui::Begin
milestone: M7b
status: open
kind: feature
area: interop
---

Planned for 0.2 by the user on 2026-11-09.

**What is missing.** A C++ namespace is a nested scope of its header's module (§15, the
`namespace A::B` row), so every call into Dear ImGui is written twice over:

```dz
imgui::ImGui::Begin("Scene")          // the dependency key, then the C++ namespace
imgui::ImGui::SliderFloat("Speed", &mut s.speed, 0, 4)
```

The user wants the namespace to stand as the module, named by the key the manifest gives
it: `imgui::Begin`, or `ImGui::Begin` with the key `ImGui`, as C++ writes it.

Nothing today shortens it. `import shp::shapes as sh` is refused with "a path reaches only
the dependencies `shp` marks `pub = true`" (checked on `tests/run/cppinterop`, 2026-11-09):
a file `import` and a manifest `via` reach a dependency's public dependencies, never into an
imported header's namespaces.

**Why it matters.** ImGui is called on every line of a panel, and most C++ libraries keep
everything in one namespace (`b2::`, `JPH::`, `fmt::`), so the doubled path is the first
thing a C++ programmer sees in Dazzle code that calls C++.

**The decision for the user: how the manifest says it.** It changes what a manifest or a
program may say, so it is a §21 row when built.

1. **A `namespace` key on the dependency**: `ImGui = { cpp = "imgui", namespace = "ImGui" }`.
   The key then names the namespace's members, and the header's global declarations (ImGui's
   backends, `ImGui_ImplSDL3_Init`) need a second key on the same recipe without
   `namespace`, or also sit under the key, with a clash between the two an error.
2. **`via` and `import` reach into an imported namespace**:
   `ImGui = { via = "imgui::ImGui" }` in the manifest, or `import imgui::ImGui as ImGui` in
   one file. No new key, and it reuses §3.2's two existing ways to name a path; the
   dependency `imgui` keeps the globals. Recommended.

Either way, `open = true` on it should put the namespace's members at the root, as for any
dependency.

| Piece | Where |
|---|---|
| A file `import`'s path, and the refusal above | `crates/dz_resolve/src/walk.rs`, near the "marks `pub = true`" message |
| A manifest `via` | `walk_via` in `crates/dz_resolve/src/lib.rs` |
| The dependency keys a manifest accepts | `KEYS` in `crates/dz_project/src/lib.rs` |
| The namespace as a nested scope of the header's module | `collect_foreign` in `crates/dz_resolve/src/lib.rs` |
| What would use it | `examples/sdl3/src/ui.dz`, every `imgui::ImGui::` |
