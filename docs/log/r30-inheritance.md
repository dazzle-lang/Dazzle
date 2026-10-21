# Struct inheritance (r30, spec 18.32)

Part of the build log in `docs/log/` — what was built and decided, kept as the record. Open work is in `docs/tasks/`; nothing here needs reading to decide what to do next.

## The plan

Designed with the user and recorded as 18.32; written into §7.3 by the revision that
implements it. `: Base` already parses and is reserved. The work, roughly in order:

1. **Layout and conversions.** The base's fields come first, so `&Derived` → `&Base` and
   `^Derived` → `^Base` are free; lower to real C++ single inheritance. A by-value
   `Derived` → `Base` is an error (no slicing). Fields and methods of the base are in
   scope in the derived type, as its own are (§7.3).
2. **Constructors.** A base with a constructor must be constructed through it:
   `Button(…) : UiElement(r) { … }`, an error when left out; a base without one is
   default-initialised (§6.2). The same rule makes a struct literal of a type with a
   constructor an error — agreed alongside, and part of the same revision.
3. **`virtual` / `override`.** Both checked in both directions; a body-less `virtual fn`
   makes the base abstract and not constructible. `drop` is virtual automatically once a
   type or a base has a `virtual` method. `UiElement::draw(self)` calls the base's version.
   Lowers to C++ `virtual`, which is also what lets a Dazzle struct inherit an imported C++
   class at M7 (Jolt's listeners) — and what M13's hot reload will have to re-point.
4. **`protected`**, a fourth visibility: the type's own methods and its derived types'.
5. **Implementations are inherited** — a `Button` is `Inspect` if `UiElement` is — and a
   more specific `impl Inspect for Button` overrides the inherited one. Static, so
   dispatch through a `&UiElement` goes to `UiElement`'s implementation; runtime dispatch
   is a `virtual` method called from it. This needs `impl_applies` to consider bases, and
   overlap to allow exactly this one shape.

**Four places in codegen a base or a `virtual` will collide with**, found while surveying
before the work started. None is hard; all four are silent if missed:

- `trivially_copyable` (`dz_check/src/lib.rs`) tests for a `drop`, a `copy` and its
  fields. A C++ class with a virtual function is **never** trivially copyable — the
  standard requires no virtual functions and no virtual bases — so a polymorphic type
  would take `Buffer<T>`'s `memcpy` path and `T::is_trivially_copyable` would fold the
  wrong way. It has to answer `false` for a type that has, or inherits, a `virtual`.
- The aggregate test in `struct_literal` (`dz_cpp`) checks for constructors and special
  members. A type with a base is not an aggregate in the sense the C++20 designated-
  initialiser path assumes, so "has a base" belongs in that test too.
- `~T()` becomes `virtual ~T()` once the type or a base has a `virtual` method, and the
  explicit `this->~T()` calls in the move and assignment paths (`dz_cpp`) want a second
  look once it does.
- `field_list` must walk bases before the type's own fields, which is also what makes the
  layout claim in step 1 true. The LSP's `methods_of` ties methods to a type by name
  (already a deferral-register row) and gets one more reason to carry the real target.

## r30 — struct inheritance (spec 18.32)

All five steps of §4's plan, and the four codegen collisions it warned about.

- **Representation.** `StructSig` gains `base`, `inherited` and `polymorphic`. A base's
  fields are **flattened into the derived type's list, base-first**, which is what makes a
  field index mean one thing everywhere — the HIR, flow analysis, codegen, the LSP — and
  what makes the layout claim true. `inherited` says where the type's own begin, so the
  C++ struct emits only those and inherits the rest. `FieldSig::declared_in` remembers
  which type wrote an inherited field, since its span is in that type's file.
  Two new passes: `collect_base` resolves `: Base` once every signature exists, and
  `flatten_bases` copies the fields in parent-first so a chain of three works from one
  pass and a cycle is reported rather than recursed into.
- **The resolver links bases too** (`Symbol::base`, `members_with_bases`), because a
  derived type's methods see the base's fields unqualified and name resolution runs long
  before the checker. It is a pass of its own between the existing two: a base is a path,
  no path resolves until every declaration exists, and every link must be made before any
  body is walked. It repeats pass 2's import step and discards what that reports.
- **Conversions.** `&Derived` → `&Base` and `^Derived` → `^Base`, checked in `dz_check`
  rather than in `dz_hir::assignable`, which has no table to walk a base chain with. A
  by-value `Derived` gets a diagnostic of its own that says what to write instead.
- **Constructors.** `: UiElement(w)` parses into `FnDecl::base_init` and is checked inside
  the constructor's scope, so its arguments see the parameters; it lands in `Body::base_init`,
  outside `root`, and becomes C++'s member-initialiser list. A base with a constructor must
  be constructed through one, and a type with a constructor — its own or inherited — is no
  longer built with a struct literal.
- **`virtual` / `override` / `protected`** are new keywords (§21 r30 says so; they were not
  reserved, so this is a real, if tiny, breaking change — `tests/run/keywords` used
  `virtual` as a method name and had to be renamed). `override` is checked against the base
  in both directions, including the signature; a body-less `virtual` is C++'s `= 0` and
  makes the type abstract, refused by value the way an imported handle already was.
  `Base::method(self)` is a new HIR callee, `BaseMethod`, emitted as C++'s qualified
  `recv.Base::method()`.
- **Implementations are inherited** by `impls_for` walking the chain, nearest-first so a
  more specific `impl` wins. `Drop` and `Copy` are deliberately excluded: they are special
  members, C++ already runs a base's destructor as part of the derived one, and inheriting
  the base's `impl Drop` would emit a second destructor running the same body twice.

**The four collisions §4 predicted, all real:**

- `trivially_copyable` now answers `false` for a polymorphic struct — a C++ class with a
  virtual function is never trivially copyable, and `Buffer<T>`'s `memcpy` path over one
  would have corrupted the vtable pointer.
- The C++20 designated-initialiser path in `struct_literal` now also excludes a type with
  a base or a `virtual`, neither of which is an aggregate in the way that path assumes.
- `~T()` is `virtual` whenever the type or a base has a `virtual` method, and a polymorphic
  type with no `drop` of its own gets `virtual ~T() = default`.
- `field_list` walks bases by construction, since the flattening did it.

One more the survey had not predicted: **a base's `drop` makes the derived type
move-only.** C++ deletes the base's copy operations, so a copy Dazzle allowed would have
been refused by the C++ compiler — a generated-code error, which invariant 7 forbids.
`base_has_drop` is asked by both `copyable` and `trivially_copyable`.
