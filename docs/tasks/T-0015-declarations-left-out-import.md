---
id: T-0015
title: Declarations left out of an import
milestone: M7b
status: open
kind: deferral
area: interop
where: `dz_cppimport`, `map_type`
---

Left out of an import, with a reason in `Header::skipped`: variadic functions and function pointers; a `wchar_t` pointer; an rvalue reference `T&&`; a `static` member function (it needs associated functions, §7); a member whose type does not follow from a template instantiation's argument binding
