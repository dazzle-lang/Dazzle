---
id: D-0077
title: Where macros live
status: decided
---

**A macro is a declaration**: private by default, `pub macro` to export, invoked as `#dep::name` from a dependency, usable before its declaration. An expansion may invoke macros, to a depth of 64, but may not declare one, so the set of macros is known before any expansion runs (§12.5)
