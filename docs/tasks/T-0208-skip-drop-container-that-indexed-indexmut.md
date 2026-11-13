---
id: T-0208
title: Skip the drop of a container that is indexed: IndexMut chosen for reads, an element lent as &mut
milestone: none
status: open
kind: feature
area: ownership
---

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
| The guard that refuses a mutating method returning `&mut`: `whole_inside`, `writes_through` | `dz_flow/src/release.rs` |
| `&mut` of an owning part counted as a replacement: `replaced` | `dz_flow/src/release.rs` |
| The test that pins what is kept today | `a_drop_is_skipped_only_when_its_region_frees_everything` in `dz_flow/tests/placement.rs` |

**What it would take.** Compiler work only (`docs/log/`). The two rows are independent; the
first is the smaller and affects far more programs.
