---
id: T-0058
title: A default argument that calls a function declared later in the module does not compile
milestone: M12
status: open
kind: deferral
area: codegen
where: `dz_cpp/src/lib.rs`, `decl_params`
---

**A default argument that calls a function declared later in the module does not compile**: C++ reads a default where the function is declared, and functions are declared in source order. Found by `tests/run/formatting`. Free functions could be declared twice — without defaults first, then with them, which C++ allows; a method's defaults cannot be added later
