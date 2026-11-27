---
id: T-0273
title: A parallel for over an element of a list does not see reads of the same element
milestone: M19
status: done
kind: bug
area: concurrency
---

**Fixed 2026-11-27**, the day it was found: the loop names its container as a local and a
path of fields **and elements**, an element by its constant index or as any element, through
the built-in index of an array or slice and through a `List`'s `Index` call alike. So
`ls[0]` beside `ls[0]` or `ls[i]` meets (DZ0433), and `ls[0]` beside `ls[1]` is apart, which
for an array of arrays the loop refused before; `tests/errors/parallel_for_over_an_element_reads_it.dz`.

**Found** by the aliasing matrix (T-0274). A data race in safe code:

```dz
let mut ls = List<List<i32>>()
ls.push(List<i32>())
parallel for &mut x in ls[0] {
    x += ls[0].len() as i32        // accepted: reads the list the passes write
}
```

The loop's check that no pass reads the container the passes write (DZ0433, §22.3) names the
container as a local and a path of fields (`reads_written`). `ls[0]` is a call of `Index`, which
has no such path, so the container is not named and nothing is compared with it. The same
with `ls[i]` beside `ls[j]`, which may be one element. A field (`p.left`) and a local are caught.

**The fix.** Name a container reached through an index too: as the element of its list (an
index proven by a constant, or any element), so a read of the same list's elements, or of the
list whole, meets it, as the block's check already does (DZ0424 catches `ls[0]` beside `ls[0]`
and `ls[i]` beside `ls[j]` in `parallel { … }`).

The matrix's cases: `loop_in_place/same_element`, `loop_in_place/elements_by_index`.

## Where the code is

| Piece | Location |
|---|---|
| Naming the container | `crates/dz_check/src/check/parallel_for.rs`: `PathStep`, `field_path`, `indexed_receiver`, `constant_index`; called from `for_stmt` in `check.rs` |
| Comparing what the body reads | `crates/dz_check/src/check/parallel_for.rs`: `reads_written` |
| The matrix | `crates/dz_driver/tests/aliasing.rs`, `KNOWN` |
