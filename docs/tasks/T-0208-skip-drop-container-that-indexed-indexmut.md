---
id: T-0208
title: Skip the drop of a container that is indexed: IndexMut chosen for reads, an element lent as &mut
milestone: none
status: done
kind: feature
area: ownership
---

**Done 2026-11-13**, both rows, in `dz_flow` alone. The first row's fix as written below would
have changed §11.2, which is `[DECIDED]` (D-0050: `index_mut` whenever the container is
writable), so the checker is unchanged: the `&mut` element `index_mut` hands back is followed
instead (`Contents::lent_to_keep`). Found on the way and fixed with it: `List::append` from
another list skipped the drop of a list that held the other list's heap strings (a leak),
`names.sort()` kept a drop it need not, and `xs[i] as usize` was refused as a view (DZ0413).
What is still kept, a local holding the element's `&mut`, is
[T-0209](T-0209-follow-element-reference-kept-in-local.md). See `docs/log/r81-ties.md`.

Left out of [T-0118](T-0118-skip-drops-that-only-free-region.md) and
[T-0145](T-0145-value-stored-into-container-built-where.md), and not one of the rows of
[T-0147](T-0147-skip-more-drops-value-lent-as.md). Found 2026-11-13.

**What it is.** A `List<String>` in a scope region, filled only with values built in that
region, still runs its drop as soon as it is indexed anywhere in the function, even for a read:

```dz
fn f() -> usize {
    let mut strings = List<String>()
    for i in 0..1000 {
        strings.push(String("x"))
    }
    return strings[500].len()       // without this line, "drop skipped"
}
```

Two separate causes:

| Case | Why its drop still runs | What would let it skip |
|---|---|---|
| any `xs[i]` on a `let mut` local, a read included | the checker picks `IndexMut` whenever the place is mutable, not when it is written, so every index is a call to `index_mut` returning `&mut String`, which `whole_inside`'s `writes_through` guard refuses | choose `IndexMut` only where the indexed place is written: assigned, borrowed `&mut`, or the receiver of a mutating method; `Index` otherwise. Codegen then calls `index` for reads too |
| `&mut xs[i]`, or `xs[i].push_str(…)` | a `&mut` to an owning element could replace it with a value from elsewhere (`*r = …`), and `replaced` cannot follow what is done through it | follow the reference: allowed when it is only the receiver of the element's own methods that never replace a part (`sealed`), and never assigned through |

Not a correctness problem: keeping a drop only costs time at scope exit (the walk over every
element, each `free` a near no-op into the region). No program prints differently.

**Where the code is.**

| Piece | Where |
|---|---|
| `Index` versus `IndexMut` for `b[i]`, decided by `place_mutability` | `ExprKind::Index` in `dz_check/src/check.rs` (around line 3480) |
| What is done with an element `index_mut` lent: `lent_to_keep`, `lends_element` | `Contents` in `dz_flow/src/release.rs` |
| A method on the container or an element, a trait's found in the type's `impl`: `keeps`, `runs` | `Contents` in `dz_flow/src/release.rs` |
| What a method may add: `adds_foreign` (`move_out` only of `self`'s own memory) | `dz_flow/src/release.rs` |
| A cast to a number carries no view | the `Cast` arm of `Cx::expr`, `dz_flow/src/ties/body.rs` |
| The tests | `an_indexed_container_skips_its_drop_unless_an_element_is_replaced` in `dz_flow/tests/placement.rs`; `element_as_number` in `tests/flow/clean.dz`, `cast_after_kill` in `tests/flow/use_after_kill.dz` |
