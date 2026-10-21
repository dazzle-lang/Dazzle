---
id: T-0034
title: A module named after a runtime-shim identifier does not compile
milestone: M12
status: open
kind: deferral
area: codegen
where: `dz_cpp/src/lib.rs`, `qualified_namespace`
---

**What goes wrong.** The runtime shim declares about forty names **directly in
`namespace dz`** — `obj`, `len`, `mv`, `str`, `index`, `Array`, `panic` and the rest — and a
module's namespace is `dz::<name>`. So a module called `obj` emits `namespace dz::obj`
beside the existing `dz::obj()`, and g++ rejects it: *"redeclared as different kind of
entity"*.

Found by `a_module_named_after_a_build_directory_does_not_collide`, which now **skips that
one name** — so the test documents the hole rather than failing on it. The same collision
already happened once for real: a module named `overflow` against the shim's `overflow`
panic helper, which was removed at r59 (`docs/log/m12.md`).

**Why it matters.** It is a name the user chose, failing for a reason that is invisible from
Dazzle: nothing in the language says `obj` is reserved, and the error arrives from g++
pointing at generated code — which invariant 7 says is a compiler bug, never a user error.
The names at risk are ordinary words, so this is not a theoretical collision; `index`, `len`
and `str` are all plausible module names in a game.

**What it would take.** Two ways, and the second is the one that cannot drift:

1. **List the shim's names in the namespace sanitiser**, so a module called `obj` emits
   `dz::obj_` or similar. Works today and breaks tomorrow: the list has to be kept in step
   with the shim by hand, and the shim grows. The `overflow` collision is what that drift
   looks like.
2. **Move the shim into `dz::rt`** and leave `dz` to module namespaces alone. Then no module
   name can ever collide, because the two live in disjoint namespaces by construction, and
   nothing has to be maintained. It touches every reference to a shim name in `dz_cpp`'s
   emitted strings, which is mechanical but broad, and it changes every codegen snapshot —
   so it is a large diff for a small defect, which is why it has waited.

Option 2 is the fix. When it lands, the skipped name in
`a_module_named_after_a_build_directory_does_not_collide` comes back, and a test naming a
module after several shim identifiers at once is cheap insurance.
