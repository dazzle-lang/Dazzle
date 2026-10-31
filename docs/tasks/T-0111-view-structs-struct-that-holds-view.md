---
id: T-0111
title: View structs: a struct that holds a view, such as an iterator
milestone: none
status: open
kind: feature
area: ownership
---

Step 2 of [D-0093](../decisions/D-0093-views-are-borrowed-no-bare-str.md), the first after
the small T-0113, by r90's order; the steps are listed in
[T-0109](T-0109-text-only-borrowed-str-const-str.md).

**First part done, r92 (2026-10-31):** `view struct`, `view` type parameters and `view type`,
their placement held to `DZ0517`, std's iterators marked, and the projection tests given
`view` (`docs/log/r81-ties.md`). **Second part, r93 (2026-10-31):** a view struct's tie on a
view field names what the field views, so `let it = xs.iter(); return it.get()` is in `xs`
(`through_view_field` in `dz_flow/src/ties/body.rs`); std's slice and text iterators
declare such ties; a union payload and a static may not hold a view. **Left:**

1. **`&T` fields in a view struct**, so the map iterators hold `&HashMap` rather than an
   untied `^HashMap`. Needs the C++ for a reference member: emitted as a pointer, read
   through at each use, since a C++ reference member cannot be reset by a move.
2. **`T::Item?` whose instance is a reference** emitted as that pointer
   ([T-0085](T-0085-optional-reference-passes-checker.md), option 3).
3. **An implementation's `in` is not held to its trait's.** `SliceIter::get` says `in
   self@items` where `Iterate::get` elides to `self`, which is narrower and so safe; but
   nothing stops an implementation naming something the trait does not, which a call
   through the trait would then trust. The same goes for `kills`, which §6.7 says may name
   no more than the trait's. A check beside the conformance check in `dz_check/src/check.rs`.
4. **A view-struct parameter taken by value is not yet a view input** to the return check
   (`caller_owned` in `dz_flow/src/ties/body.rs`), so returning what it views is refused.

**What it is.** A struct holding a view (`Chars`'s `m_text: str`, `SliceIter`'s
`m_slice: []T`) is tied as a whole today, but nothing stops one being stored. Once views are
borrowed (T-0109, T-0110), a struct could not hold one at all (`DZ0517`), yet an iterator
must. So this lands first, while `str` and `[]T` are still values, and those two tasks then
only respell the fields. A struct declared `view struct` may hold references, and is itself
a view: it lives only where a reference does, carries one tie for everything its fields
view, and elision and `in` apply to it as to a reference. A method returning from inside it
names what the field views, so its result outlives the iterator:

```dz
pub view struct SliceIter<T> {
    m_slice: &[]T
    m_index: usize = 0
    tie @items = m_slice           // what the slice views

    pub fn next(&mut self) -> ^T in self@items { … }
}

fn first(xs: &List<i32>) -> ^i32 {
    let it = xs.iter()             // in `xs`
    return it.next()               // in `xs` too, not in the local `it`
}
```

**Settled in r90.** Generic code over iterators needs `view` too, or D-0093's point 8 refuses
`T = SliceIter<i32>`: a type parameter written `view T`, or an associated type a trait
declares `view type`, may be a view, and the generic body checks a value of it as one. A
plain `T` never is.

```dz
pub trait Iterate { view type Item … }
pub trait Iterable { view type Iter: Iterate … }
fn count<view T: Iterate>(it: T) -> usize { … }
view struct Numbered<view I: Iterate> { pub inner: I, … }
```

- **`view` is a keyword only before `struct`, `type` and a type parameter.** The lexer is
  unchanged and `view` stays an identifier everywhere else: `let view = Mat4::look_at(…)`
  (`tests/run/math`) is a view matrix, and three other test programs name things `view`.
- **A struct with a `view` type parameter is a `view struct`**, and a `view` parameter passes
  only to another `view` parameter.
- **`T::Item?` whose instance is a reference is that pointer**: `^i32` for a `SliceIter<i32>`,
  `Token?` for an iterator that makes `Token`s ([T-0085](T-0085-optional-reference-passes-checker.md),
  its option 3, for projections only, since a plain `T` is never a reference).
- **Revises §5.2 and D-0003** when it lands: a `&T` field is allowed in a view struct, which
  is placed as a reference is.

An optional, tuple or array of views is a view by the same rule, with no marker.

**Why it matters.** It is how `SliceIter`, `HashMapIter`, `SlotMapIter`, `Split`, `Chars`
and a user's own iterators keep their tie; r88 tied a returned aggregate as a whole, which
this makes a type's declared property. A view-struct parameter taken by value is a view
input, which closes r88's exemption for a result coming out of a by-value parameter that
holds a view. The census's "general iterators" item goes with it.

**Why it is this way.** The marker is explicit, by D-0093, so a struct never becomes
unstorable because someone added a field.

**What it would take.** A language change (§5.2, §5.6, §8.5, §20, a §21 row): `view` before
`struct`, `type` and a type parameter (contextual, r90); `DZ0517` relaxed inside a view
struct and extended to a view struct, or a value of a `view` parameter or projection, used
where a reference may not go; a `tie` standing for a reference field; a path through it read
in `dz_flow` as what the struct value views. r88's handling of aggregates in `bind`,
`tuple_part` and `LetTuple` becomes the general case. The projection tests gain `view`:
`tests/run/projection` (`count`, `drain`, `Numbered`), `typed/`'s `first_item` and
`Flagged`, and spec §8.5's examples.

| Piece | Where |
|---|---|
| The iterators | `dz_project/src/std/collections/{slice_iter,slice_iter_mut,hash_map_iter,hash_map_iter_mut,slot_map_iter,slot_map_iter_mut}.dz`; `text/chars.dz`; `Split` in `text/text.dz` |
| What holds a view | `holds_view` in `dz_check/src/ties.rs`; `contains_views` in `dz_flow/src/ties.rs` |
| A struct local's views, a returned aggregate | `bind`, `tuple_part`, `LetTuple` in `dz_flow/src/ties/body.rs` |
| A `tie`'s paths | `export_path`, `tie_decls` in `dz_check/src/ties.rs` |
| `for` over a container | `iterated_container`, `iterated_origins` in `dz_flow/src/ties/body.rs` |
| `DZ0517` | `dz_check/src/lib.rs` (`REF_IN_FIELD`) |
| r92's syntax: `StructDecl::is_view`, `GenericParam::is_view`, `MemberKind::TypeAlias::is_view` | `dz_ast/src/lib.rs`; parsed in `dz_parse/src/item.rs` (`parse_struct`, `parse_generics`, `parse_member`, the item dispatch), printed by `dump.rs` and `dz_fmt` (`generics_str`, `member`, `item`); coloured in `editors/vscode/syntaxes/dazzle.tmLanguage.json` |
| r92's checks: `ty::Param::view`; `TypeTable::is_view_struct` (filled in pass 0, `collect_type_params`, which also refuses a `view` parameter on a plain struct, a union or a trait) and `placed_as_view`; a field in `collect_struct`; a written type argument in `check_view_arg` (from `type_args`); an inferred one in `check_bounds` (`check.rs`); an implementation's parameter in `impl_header` (`impls.rs`); an associated-type choice in the conformance check (`check.rs`, `AssocType::is_view`) | `dz_hir/src/ty.rs`, `dz_check/src/lib.rs`, `check.rs`, `impls.rs` |
| The traits whose associated types become `view type` | `dz_project/src/std/core/iterate.dz` (`Iterate`, `Iterable`, `IterableMut`) |
| `T::Item?` emitted as a pointer for a reference item | `cpp_ty` in `dz_cpp/src/lib.rs`; the projection's lowering in `dz_check` |
| The generic iterator code that gains `view` | `tests/run/projection/src/main.dz`, `tests/workspace/typed/src/typed.dz` (`Flagged`), spec §8.5 |
