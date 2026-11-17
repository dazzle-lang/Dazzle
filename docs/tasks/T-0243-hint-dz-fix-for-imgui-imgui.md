---
id: T-0243
title: A hint and a dz fix for imgui::ImGui::x where ImGui is named bare
milestone: none
status: open
kind: feature
area: llm-tooling
where: `lift_namespaces` in `crates/dz_resolve/src/lib.rs` (which names a module lifts, in `ModuleNames.picks`); `resolve_head` and `check_members` in `crates/dz_resolve/src/walk.rs` (where a path through a dependency key reaches a namespace); `cmd_fix` in `crates/dz/src/main.rs` and the fixes `dz fix` applies
---

Planned for 0.3 by the user on 2026-11-17, when `dz fix` and the other tools are worked on.

**What is missing.** Since r141 (D-0109, T-0179) a `cpp` dependency's `namespaces` names a
header's namespace bare, `ImGui::Begin`, and the full path `imgui::ImGui::Begin` still
works. Nothing tells a program that still writes the full path that the short one exists.
The compiler knows: the path's first two segments are a dependency key and a namespace
that the same manifest lifts. It should say so, as a hint (not a warning: the full path is
valid and stays so), with a fix that drops the key, `imgui::ImGui::Begin` to
`ImGui::Begin`, which `dz fix` applies and the editor offers as a quick fix.

**Why it matters.** Code written before a manifest lifted the namespace, and code an LLM
writes from older examples, keeps the doubled path; a hint with a fix makes the short form
the one a project converges on without anyone searching for it.

**What it would take.** At a path whose head is a dependency key and whose next segment
reaches a namespace the module lifts (the same symbol in `ModuleNames.picks` under that
name), a diagnostic of a new code with a machine-applicable fix deleting `key::`. The
`[names]` choice of a declaration, `Vec2 = "std"` with `std::Vec2` written, is the same
situation and could share it.
