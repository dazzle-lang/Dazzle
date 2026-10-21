---
id: D-0033
title: `move` on a copyable value
status: decided
---

**It moves.** The source is moved-from, a later use is an error, and it is reset to its defaults at runtime — the same as for any type. The compiler had been treating it as a copy, which §6.4 never said; one rule for every type is simpler to learn, and it keeps a generic body meaning one thing for every argument. To keep using a copyable value, leave out `move` (§6.4)
