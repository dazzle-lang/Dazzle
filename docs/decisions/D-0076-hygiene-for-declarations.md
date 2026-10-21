---
id: D-0076
title: Hygiene, for declarations
status: decided
---

**Locals an expansion introduces are hidden; declarations it introduces are not.** A `let`, parameter or binding written in `expand` cannot capture or be captured; types, functions, methods, fields, variants and constants are ordinary declarations the caller names, and a type's members are reachable bare from its methods as usual. Other names in `expand` resolve where the macro is declared. No identifier pasting. Refines §12.4's rule, which read literally hid a generated `union States` from the code it was generated for (§12.4)
