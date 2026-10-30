---
id: T-0111
title: View structs: a struct that holds a view, such as an iterator
milestone: none
status: open
kind: feature
area: ownership
---

Step 3 of [D-0093](../decisions/D-0093-views-are-borrowed-no-bare-str.md); the steps are
listed in [T-0109](T-0109-text-only-borrowed-str-const-str.md).

**What it is.** Once views are borrowed (T-0109, T-0110), a struct cannot hold one (`DZ0517`),
yet an iterator must. A struct declared `view struct` may hold references, and is itself a
view: it lives only where a reference does, carries one tie for everything its fields view,
and elision and `in` apply to it as to a reference. A method returning from inside it names
what the field views, so its result outlives the iterator:

```dz
pub view struct SliceIter<T> {
    m_slice: &[]T
    m_index: usize = 0
    tie @items = m_slice           // what the slice views

    pub fn next(&mut self) -> &T? in self@items { … }
}

fn first(xs: &List<i32>) -> &i32? {
    let it = xs.iter()             // in `xs`
    return it.next()               // in `xs` too, not in the local `it`
}
```

An optional, tuple or array of views is a view by the same rule, with no marker.

**Why it matters.** It is how `SliceIter`, `HashMapIter`, `SlotMapIter`, `Split`, `Chars`
and a user's own iterators keep their tie; r88 tied a returned aggregate as a whole, which
this makes a type's declared property. A view-struct parameter taken by value is a view
input, which closes r88's exemption for a result coming out of a by-value parameter that
holds a view. The census's "general iterators" item goes with it.

**Why it is this way.** The marker is explicit, by D-0093, so a struct never becomes
unstorable because someone added a field.

**What it would take.** A language change: `view` before `struct` (a keyword or contextual,
to settle), `DZ0517` relaxed inside a view struct and extended to a view struct used where
a reference may not go; a `tie` standing for a reference field; a path through it read in
`dz_flow` as what the struct value views. r88's handling of aggregates in `bind`,
`tuple_part` and `LetTuple` becomes the general case.

| Piece | Where |
|---|---|
| The iterators | `dz_project/src/std/collections/{slice_iter,slice_iter_mut,hash_map_iter,hash_map_iter_mut,slot_map_iter,slot_map_iter_mut}.dz`; `text/chars.dz`; `Split` in `text/text.dz` |
| What holds a view | `holds_view` in `dz_check/src/ties.rs`; `contains_views` in `dz_flow/src/ties.rs` |
| A struct local's views, a returned aggregate | `bind`, `tuple_part`, `LetTuple` in `dz_flow/src/ties/body.rs` |
| A `tie`'s paths | `export_path`, `tie_decls` in `dz_check/src/ties.rs` |
| `for` over a container | `iterated_container`, `iterated_origins` in `dz_flow/src/ties/body.rs` |
| `DZ0517` | `dz_check/src/lib.rs` (`REF_IN_FIELD`) |
