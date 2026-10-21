---
id: T-0039
title: A trait method's non-constant default argument
milestone: M12
status: open
kind: deferral
area: codegen
where: `dz_cpp`, `trait_object_decls`
---

**A trait method's non-constant default argument** is rendered on the handle's declaration, before the module's structs are defined, so a default like `Vec3{}` on a trait method would not compile there
