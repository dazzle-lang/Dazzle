---
id: T-0109
title: Text is borrowed: &str and const &str, and no view as a type argument
milestone: none
status: open
kind: feature
area: language
---

**Next, by the user's choice (2026-10-30).** Step 1 of
[D-0093](../decisions/D-0093-views-are-borrowed-no-bare-str.md) (r89, decided, not yet
implemented), which settles how the ties of D-0091 become complete. The steps, in order,
each landing on its own:

1. **This task:** `&str`, `const &str`, and no view as a type argument.
2. [T-0110](T-0110-slices-are-borrowed-t-mut-t.md): slices are `&[]T`, `&mut []T`,
   `const &[]T`.
3. [T-0111](T-0111-view-structs-struct-that-holds-view.md): view structs, and the iterators.
4. [T-0112](T-0112-cstr-behind-pointer-text-at-c.md): `cstr` behind a pointer, and text at
   the C and C++ boundary.
5. [T-0113](T-0113-function-types-write-in-on-their.md): function types write `in`.
6. [T-0114](T-0114-untied-pointer-becomes-reference-only-in.md): an untied pointer becomes a
   reference only in `unsafe`.

**What it is.** `str` is today a value type, a `{ ptr, len }` view that can be stored in a
field or a container (`List<str>`). It becomes `&str`, and r87's `const str` becomes
`const &str`; a bare `str` is no longer a type a value can have. No view may be a type
argument, extending `DZ0517` (no reference in a field):

```dz
// today
fn name(&self) -> str in self@text
let names = List<str>()
fn label() -> const str

// after
fn name(&self) -> &str in self@text
let names = List<const &str>()      // List<&str> is an error
fn label() -> const &str
```

A `String` passes where a `&str` is wanted through `ImplicitAs<&str>`, at arguments only,
borrowed and tied to the `String` (D-0093, point 7).

**Why it matters.** A `str` is a reference that does not look like one, and every rule about
references has had to special-case it. With it gone and no view as a type argument, no `T`
is ever a view, so r88's exemption for results that are views only through `T` (`pop() ->
T?`) disappears and generic bodies are fully checked: the census in `docs/log/r81-ties.md`
counts about 80 sites left unchecked by it. An LLM trained on Rust also reads `&str` right.

**Why it is this way.** `str` was designed as a value view (D-0050, D-0092) before ties
existed.

**What it would take.** A language change: r89's row records the decision, and this step
revises §5.3, §6.7 and §20 and adds its own §21 row. Every `str` in std, the examples and
the corpora changes (about 150 mentions in std, 260 in `tests/`, 17 containers of `str`).

| Piece | Where |
|---|---|
| The ties code as a whole | T-0107's table ([T-0107](T-0107-design-ties-kills-clauses-compiler-placed.md)) |
| `Ty::Str`, `Ty::ConstStr`; `assignable` letting a `const str` be a `str` | `dz_hir/src/ty.rs` |
| `const` before a type: `TypeKind::Const`, lowered in `lower_type_inner` (`DZ0569`) | `dz_parse/src/ty.rs`; `dz_check/src/lib.rs` |
| A literal becoming a `const str`, read as a `str` | `expr_inner` / `expr` in `dz_check/src/check.rs` |
| `DZ0517`, a reference in a field | `dz_check/src/lib.rs` (`REF_IN_FIELD`) |
| `ImplicitAs` at arguments | `dz_check/src/check.rs` (`implicit_as`, near line 5508); std's in `core/implicit_as.dz`, `text/string.dz` |
| r88's generic exemption, to remove | `untied_result` in `dz_check/src/ties.rs`; `Signature::effects`, `generic_result` in `dz_flow/src/ties/body.rs` |
| C++ emission, unchanged (`::dz::str`) | `dz_cpp` |
