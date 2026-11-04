---
id: D-0046
title: State that lives for the whole program
status: decided
---

**`static`, declared at module scope, with a written type and a constant initialiser; `#[thread_local]` for one per thread.** Reading is safe, changing is `unsafe` (revised by D-0094 point 7, r112: changing one is safe, and a `kills` clause names what it kills in one), nothing moves out. Needed by the standard allocators — Temp's arena and the tracking tables are state no function owns — rather than by Global's speed: Global stays `malloc`. A static built by arbitrary code would depend on C++'s unordered initialisation across files, so the initialiser is constants only (§7.7, §14.2)

**Revised by D-0094 (r113)**: a static is dropped at exit in an order a drop can rely on, a module's statics before those of the modules it uses and in reverse order of declaration within one, and is left at its default. C++ destroyed statics in different files in no promised order, so one's drop could read another already freed (T-0163).
