---
id: T-0013
title: Hover on an imported name
milestone: M7a
status: open
kind: deferral
area: editor
where: `dz_lsp/src/ide.rs`, `hover`
---

**What goes wrong.** Hovering an imported C or C++ name — `rl::DrawRectangle`,
`rl::Rectangle` — shows **nothing**. The editor's hover text is built from *Dazzle*
declarations: `ide.rs` renders a signature from the AST of the file that declares the name,
and an imported symbol has no such declaration. It came from a header.

**Go-to-definition should already work**, because a symbol's span points into the header
file — but that is untested, and worth confirming before anything else, since if it does
work it is already most of the value.

**Why it matters.** It matters more for imported names than for Dazzle ones, because the
imported signature is **not the C signature the programmer knows**. §15.2 maps pointers,
references, handles and callbacks, so `DrawRectangle`'s Dazzle shape is something the
programmer has to discover — and hover is exactly where they would look. This is the same
gap [T-0061](T-0061-api-dump-dz-api-module.md) describes for a model (`dz api rl`, showing an
imported module *as Dazzle*): the information exists in the type table and nothing presents
it.

**What it would take.** The imported symbol has everything needed — its parameter types,
return type and name are in the type table, which is how calls to it are checked. So the
work is a second rendering path in `hover`: given a symbol with no Dazzle AST, render its
signature from the table rather than from source. That is the same rendering
[T-0061](T-0061-api-dump-dz-api-module.md) needs, so the two should share one function and be
done together — one formatter, used by hover, by `dz api`, and by completion detail.

Two things worth adding while there, since they are the parts a C programmer actually wants:
the **header file and line** the declaration came from, and the **original C spelling**
beside the Dazzle one, so the mapping §15.2 performs is visible rather than inferred.
Doxygen comments from the header would be better still, and libclang can supply them
(`clang_Cursor_getParsedComment`) — worth checking whether the importer could keep them at
no real cost.
