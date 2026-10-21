---
id: D-0075
title: How a macro is written
status: decided
---

**Captures read like parameters and their shapes like types:** `$name: kind`, `$xs: []K`, `$xs: [,]K`, `$x: K?`, `$( … )?` for an optional group; built-in kinds lower-case (`ident`, `literal`, `type`, `expr`, `stmt`, `block`, `code`, `decl`), `syntax` rules `PascalCase`; macro kinds `decl`, `stmt`, `expr`, sharing those names. `expand` is shaped by `#for v in $xs[,] { … }`, `#if $x { … } #else { … }` and `#error`, reusing `for` and §12.10's `#if` rather than a notation of its own. Replaces r1's `$name:#name`, postfix `[]`, `(…)?` groups and `$xs[,] { $.f }`, whose `#` on a kind collided with invocations and whose bare parentheses could not be told from literal ones. Matching never backtracks (§12.1–§12.3)
