---
id: D-0079
title: Diagnostics in an expansion
status: decided
---

**Where the code was written:** in captured code at the user's own text; in code the macro wrote at the invocation's name, with an "in expansion of" note. A mistake in a capture copied to several places is reported once, a parameter copied to several places is unused only if every copy leaves it unused, and a warning about a macro's own local is not shown to its user (§12.6)
