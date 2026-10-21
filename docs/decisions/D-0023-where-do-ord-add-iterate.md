---
id: D-0023
title: Where do `Ord`, `Add`, `Iterate` and `Format` live before the stdlib?
status: decided
---

**An implicit prelude written in Dazzle**, module `dz`, carried as source by the compiler and in scope everywhere with no import. Not compiler-invented symbols: the trait machinery is then exercised by real Dazzle code from the first day it exists, and at M12 the prelude grows into the stdlib instead of being deleted (§3.2). *Superseded by 18.57:* the prelude became the module `std`, named in the manifest like any dependency
