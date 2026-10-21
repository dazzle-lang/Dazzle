---
id: T-0014
title: The import cache is in-process only
milestone: M7a
status: open
kind: deferral
area: interop
where: `dz_cppimport`, `cache`
---

The import cache is **in-process only**. §15.1 wants it on disk too, so `dz check` does not re-parse an unchanged header; the LSP, which matters most, is covered
