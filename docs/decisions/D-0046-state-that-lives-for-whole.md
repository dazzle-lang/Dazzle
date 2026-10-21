---
id: D-0046
title: State that lives for the whole program
status: decided
---

**`static`, declared at module scope, with a written type and a constant initialiser; `#[thread_local]` for one per thread.** Reading is safe, changing is `unsafe`, nothing moves out. Needed by the standard allocators — Temp's arena and the tracking tables are state no function owns — rather than by Global's speed: Global stays `malloc`. A static built by arbitrary code would depend on C++'s unordered initialisation across files, so the initialiser is constants only (§7.7, §14.2)
