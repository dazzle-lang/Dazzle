---
id: T-0032
title: A name an expansion resolves where the macro is declared must be `pub` there
milestone: M10
status: open
kind: deferral
area: macros
where: `dz_macro/src/hygiene.rs`, `qualify`
---

**A name an expansion resolves where the macro is declared must be `pub` there.** §12.4 resolves the macro's own names in its module; the expander does it by qualifying them with the caller's alias (`engine::Tally`), which reaches only what that module exports — not its private helpers, and not names it reaches through its own dependencies (`rl::…` in the engine)
