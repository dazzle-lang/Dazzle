---
id: T-0018
title: Inheritance between imported C++ classes
milestone: M7b
status: open
kind: deferral
area: interop
where: `dz_cppimport`, `collect_methods`; `dz_check/src/lib.rs`, `collect_base`
---

**What goes wrong.** When `import cpp` reads a C++ class hierarchy, each class arrives
**flat and unrelated**: a base's fields and methods are not imported into a derived class,
so a `Derived` has only what its own declaration lists, and `&Derived` does not convert to
`&Base`. Naming an imported class as a base from the Dazzle side — `struct MyListener :
jolt::ContactListener` — is refused by `collect_base` with `DZ0541`.

**Why a game needs it.** **Jolt's shapes, bodies and listeners are class hierarchies**, and
so are most C++ engine libraries: the useful methods live on the base and the thing you hold
is the derived type. With the hierarchy flattened away, calling a base method means reaching
for a cast that does not exist, and the library is usable only through whatever parts of its
API happen to be declared on the concrete classes.

**No longer blocked.** This was waiting on Dazzle having inheritance at all, and **struct
inheritance landed at r30** — so the Dazzle side already exists: a `Symbol::base`, a
flattened field list, and the `&Derived` → `&Base` conversions.

**What is left is on the importer.** Three pieces:

1. `collect_methods` reads a `CXCursor`'s own members; it needs to follow
   `clang_visitChildren`'s base specifiers and fill in `Symbol::base` with the imported base
   — then the existing flattening does the rest.
2. `collect_base` must accept an imported class as a base rather than reporting `DZ0541`,
   which is what lets a Dazzle struct sit in a C++ hierarchy.
3. The conversions must emit as C++ conversions rather than Dazzle's own, since the layout
   is the header's: a `static_cast` where the base is non-virtual, and nothing at all where
   it is the first base — but **virtual** and **multiple** inheritance shift the pointer,
   and Jolt uses both. Deciding what is supported is part of the work: the honest first cut
   is single non-virtual inheritance, with a diagnostic naming the others rather than
   emitting a cast that is quietly wrong.

Related: [T-0009](T-0009-dazzle-struct-implementing-imported-c-abstract.md), which is this
plus virtual dispatch, and is the form Jolt's `ContactListener` actually needs;
[T-0010](T-0010-proof-on-real-libraries-dear-imgui.md), which is how either gets proven.
