---
id: T-0017
title: An imported overloaded method is not cast at the boundary
milestone: M7b
status: open
kind: deferral
area: codegen
where: `dz_cpp/src/lib.rs`, `call`
---

An imported overloaded method is not cast at the boundary the way a free function is: `hir::Callee::Method` carries the name, not the overload key, so `dz_cpp` applies `c_cast`/`ret_cast` only where the name names one method
