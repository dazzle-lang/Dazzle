---
id: T-0020
title: Libclang parses with its own default target
milestone: M7b
status: open
kind: deferral
area: interop
where: `dz_cppimport`, `arguments`
---

libclang parses with its own default target (MSVC here), not the C++ compiler's: a header needing the C library's own headers may find MSVC's while g++ builds against mingw's
