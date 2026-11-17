---
id: T-0179
title: A C++ namespace reached as a module of its own: ImGui::Begin, not imgui::ImGui::Begin
milestone: M7b
status: done
kind: feature
area: interop
where: `LiftedNamespace` and `lifted_namespaces` in `crates/dz_project/src/lib.rs` (the manifest key); `lift_namespaces` in `crates/dz_resolve/src/lib.rs` (a pick per namespace, and the collisions); the type-path branch of `crates/dz_check/src/lib.rs` that walks `module::Type` (a path starting at a bare namespace); tests in `crates/dz_driver/tests/namespaces.rs` and `tests/run/cppinterop`
---

Planned for 0.2 by the user on 2026-11-09.

**Done 2026-11-17, the user's design (D-0109, r141), which is neither option below.** The
dependency lists the namespaces to name bare: `imgui = { cpp = "imgui", namespaces =
["ImGui"] }` makes `ImGui::Begin` mean `imgui::ImGui::Begin`, and the key keeps the
header's globals, `imgui::ImGui_ImplSDL3_Init`. A nested namespace is named by its last
segment. Each lifted namespace is a `[names]`-style choice in the module's `ModuleNames.picks`,
so resolution, the checker and the editor follow one rule. A name already used in the
module, by a dependency key, a declaration, a `[names]` choice or another listed namespace,
is `DZ0318` (new) on the manifest line; a namespace the header lacks is `DZ0301`; and
`namespaces` on a non-`cpp` dependency is `MANIFEST_BAD_VALUE`. `examples/sdl3/src/ui.dz`
now writes `ImGui::` on its 38 calls, and the panel draws as before.

Found on the way: a path starting at a namespace named bare, by this or by an opened header
(`open = true`), resolved but did not type a struct literal or an annotation,
`shapes::Point{ x: 1, y: 2 }`: the checker's type-path branch only walked paths whose head
was a dependency key. It now walks one whose head is a namespace too.

The hint the user asked for next, pointing at `imgui::ImGui::x` where `ImGui` is named bare
with a `dz fix` dropping the key, is [T-0243](T-0243-hint-dz-fix-for-imgui-imgui.md), for 0.3.

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
