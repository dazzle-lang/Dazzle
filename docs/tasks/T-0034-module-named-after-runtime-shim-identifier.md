---
id: T-0034
title: A module named after a runtime-shim identifier does not compile
milestone: M12
status: open
kind: deferral
area: codegen
where: `dz_cpp/src/lib.rs`, `qualified_namespace`
---

**A module named after a runtime-shim identifier does not compile.** The shim declares about forty names directly in `namespace dz` — `obj`, `len`, `mv`, `str`, `index`, `Array`… — and a module’s namespace is `dz::<name>`, so a module called `obj` emits `namespace dz::obj` beside the existing `dz::obj()` and g++ rejects it as "redeclared as different kind of entity". Found by `a_module_named_after_a_build_directory_does_not_collide`, which now skips that one name. Listing the shim’s names in the namespace sanitiser would work but drifts as the shim grows; moving the shim into `dz::rt` and leaving `dz` to module namespaces alone is the fix that cannot drift
