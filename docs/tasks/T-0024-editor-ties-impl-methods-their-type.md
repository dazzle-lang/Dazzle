---
id: T-0024
title: Editor ties `impl` methods to their type by name
milestone: M9
status: open
kind: deferral
area: editor
where: `dz_lsp/src/ide.rs`, the outline query
---

**What goes wrong.** The outline query groups an `impl` block's methods, and a trait's
inherited defaults, under their type by **matching the type's name as written**. Two types
with the same name in different modules therefore look like one to the editor: an
`impl Format for Vec2` in `engine` and another in `ui` would have their methods pooled under
a single "Vec2", and the outline, and anything built on it, would cross them.

**Why it matters.** Not for correctness — the **compiler** is not confused; this is the
editor's own index, so the worst case is a wrong outline entry or a wrong symbol in a
workspace-symbol search, not a wrong program. But it is exactly the kind of defect that
survives for a long time and then presents as "go to definition took me to the wrong file",
which is hard to explain and erodes trust in the whole language server. And it gets more
likely as a project grows: `Vec2`, `Transform`, `Node` and `State` are names every module
reaches for.

**Why it is this way.** Convenience: the outline is built from the AST, where an `impl`'s
target is a path as written, and resolving it means going through the resolved symbol table
instead. Nothing forced that until there were two same-named types.

**What it would take.** Little, and the information is already there: **the checker knows
each implementation's real target** — `ImplSig` carries the resolved `self_ty`, which is what
overlap checking and method lookup use. So the outline should key on the `SymbolId` rather
than on the name, and render the name only for display. That is a change of key, not a change
of mechanism.

The test is a `tests/workspace` fixture with two modules each declaring a `Vec2` and each
implementing a trait for it, asserted through the `ide.rs` query directly — those queries
take an `Analysis` and an offset and are tested without a client, so no editor is involved.
