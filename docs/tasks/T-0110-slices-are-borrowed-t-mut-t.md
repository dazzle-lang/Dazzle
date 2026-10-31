---
id: T-0110
title: Slices are borrowed: &[]T, &mut []T and const &[]T
milestone: none
status: done
done: r99, 2026-10-31
kind: feature
area: language
---

**Done, r99 (2026-10-31),** as T-0109 was: `&[]T` and `&mut []T` are still `Ty::Slice`, a new `Ty::ConstSlice` is `const &[]T` (assignable to `&[]T`, read as one where it is not wanted, the same C++ type), and `^[]T` is an optional slice, empty as `null` (`view_pointer` in `dz_check/src/check.rs`). `DZ0577` respells the old forms; `placed_as_view` counts a slice; a `&mut []T` is a mutable input for `kills`. Reflection's tables are `const &[]`. **Left:** a `const &[]T` cannot yet be written by a program: `const PRIMES: const &[]i32 = &[2, 3, 5, 7]` needs the literal emitted as static storage, since a `const` is evaluated where it is named. Slicing a `const` array, `PRIMES[..]`, gives a `&[]T`.

Step 4 of [D-0093](../decisions/D-0093-views-are-borrowed-no-bare-str.md), after
[T-0109](T-0109-text-only-borrowed-str-const-str.md), which lists every step.

**What it is.** A slice is a view exactly as `str` is, and gets the same treatment: `[]T`
becomes `&[]T`, `[]mut T` becomes `&mut []T`, and a static table is `const &[]T`. Fixed
arrays `[N]T` stay values.

```dz
fn total(xs: []i32) -> i32          // today
fn total(xs: &[]i32) -> i32         // after
const PRIMES: const &[]i32 = &[2, 3, 5, 7]
```

**Why it matters.** Left a value, a slice keeps the hole `str` closes in T-0109: `List<[]T>`,
a struct holding a slice, and a `T` that is a view. It also answers T-0107's open question
of whether `const []T` follows `const str`: static tables come with it.

**Why it is this way.** Slices were designed as values with text (D-0050).

**What it would take.** A language change (§5.3, §6.7, §20, a §21 row). Every `[]T` in std,
the examples and the corpora; `..xs` variadics, whose parameter is a `[]T` today
(`FnSig::variadic`); slicing `xs[a..b]`, which now yields a `&[]T`. The iterators that hold a
slice (`SliceIter`, `SliceIterMut`) are view structs by then, since
[T-0111](T-0111-view-structs-struct-that-holds-view.md) lands first (r90), so their `m_slice`
simply becomes `&[]T` and `&mut []T`. A slice that may be absent is `^[]T`, as text is
`^str` (r90). Reflection's tables become static: `TypeInfo::fields` and `variants` and
`VariantInfo::fields` are `const &[]FieldInfo` and `const &[]VariantInfo`.

| Piece | Where |
|---|---|
| `Ty::Slice` | `dz_hir/src/ty.rs` |
| Reflection's slices | `dz_project/src/std/reflect/{type_info,variant_info}.dz` |
| The variadic parameter | `FnSig::variadic`, `variadic_elem` in `dz_check/src/lib.rs` |
| `const` lowering, which today accepts only `str` (`DZ0569`) | `lower_type_inner` in `dz_check/src/lib.rs` |
| The ties code as a whole | [T-0107](T-0107-design-ties-kills-clauses-compiler-placed.md)'s table |
