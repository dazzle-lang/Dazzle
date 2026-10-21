---
id: T-0016
title: An overloaded free function imports only its first declaration
milestone: M7b
status: open
kind: deferral
area: interop
where: `dz_cppimport`, `function`
---

**What goes wrong.** C++ lets a free function be overloaded — several declarations sharing
one name, told apart by their parameters. When `import cpp` reads a header,
`dz_cppimport`'s `function` lets the **first** declaration claim the name and passes over
the rest. They are not reported as skipped and not reachable by any spelling; they simply are
not there.

Overloaded **methods** do resolve by argument type (§15.3), so this is specifically the
free-function case.

**Why a game needs it.** **Dear ImGui's C++ API is built on them.** `ImGui::Checkbox`,
`Combo`, `BeginChild`, `TreeNode`, `PushID` and many more have several overloads each, and
today only the first declaration of each imports — which for most of them is not the one you
want. Until this lands, ImGui is usable from Dazzle only through **cimgui**, its C wrapper,
which means an extra vendored dependency and an API a generation behind the C++ one. ImGui
is how a game gets a debug UI, so this is the gap between "can inspect the game while it
runs" and "cannot".

**What it would take.** Two possible spellings, and the choice is the user's:

1. **Resolution by argument type**, as imported methods already do. The importer records an
   overload set under one name and the checker scores a call against it. Consistent with
   §15.3 and needs no new syntax — but it is function overloading in all but name, which
   D-0005 keeps out of the *language*; the argument for allowing it here is that these are
   not Dazzle declarations, they are a C++ header's, and refusing to see half of a header is
   not a design choice anybody gains from.
2. **The `@(...)` disambiguation suffix**, which §15 lists as `[PROPOSED]` and which is
   **unparsed** today — `ImGui::Checkbox@(cstr, ^mut bool)` naming one overload explicitly.
   Keeps D-0005 intact and makes the choice visible at the call site, at the cost of
   verbosity on the most-called functions in the header.

Both need the importer to stop discarding: `Header::skipped` should at minimum record the
passed-over declarations with a reason, so that `dz api` ([T-0061](T-0061-api-dump-dz-api-module.md))
and a diagnostic can say "there are three more `Checkbox`es and you cannot reach them"
rather than leaving the user to wonder why the arguments do not match.

Related: [T-0017](T-0017-imported-overloaded-method-not-cast-at.md) (an imported overloaded
method is not cast at the boundary), [T-0015](T-0015-declarations-left-out-import.md) (what
else an import leaves out).
