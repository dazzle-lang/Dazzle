---
id: T-0016
title: An overloaded free function imports only its first declaration
milestone: M7b
status: open
kind: deferral
area: interop
where: `dz_cppimport`, `function`
---

**An overloaded free function imports only its first declaration** — `claim` takes the name and the rest are passed over. Overloaded *methods* do resolve by argument type (§15.3). The `@(...)` disambiguation suffix is `[PROPOSED]` and unparsed

**Why a game needs it** (from the readiness review, 2026-09-14):

**Overloaded free functions** (M7b). **Dear ImGui's C++ API is built on them** —
`ImGui::Checkbox`, `Combo`, `BeginChild`, `TreeNode`, `PushID` and more have several
overloads, and today only the first declaration of each imports. Until this lands ImGui is
usable only through cimgui, its C wrapper. Needs the `@(...)` disambiguation suffix
(`[PROPOSED]`) or resolution by argument type, as methods already do.
