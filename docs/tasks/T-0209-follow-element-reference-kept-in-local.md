---
id: T-0209
title: Follow an element reference kept in a local: let r = xs[i], for &mut s in xs
milestone: none
status: open
kind: feature
area: ownership
---

Left out of [T-0208](T-0208-skip-drop-container-that-indexed-indexmut.md), which follows the
`&mut` element `index_mut` hands back only as far as the expression it is used in. Found
2026-11-13.

**What it is.** A `List<String>` in a scope region, filled only with values built there, still
runs its drop when a local holds a writable reference to one of its elements, even if nothing
is ever assigned through it:

```dz
fn kept() -> usize {
    let mut names = List<String>()
    names.push(String::from("x"))
    let r = names[0]            // a `&mut String`: `index_mut` (§11.2)
    r.push_str("!")
    for &mut s in names {       // `iter_mut`
        s.push_str("?")
    }
    return names.len()          // the drop is kept
}
```

| Case | Why its drop still runs | What would let it skip |
|---|---|---|
| `let r = xs[i]`, `let r = &mut xs[i]` | `Contents::lent_to_keep` refuses a `let` of something that writes through | follow the local: a reference cannot be pointed elsewhere, so every use of `r` is a use of the element, judged as `lent_to_keep` judges the expression |
| `for &mut s in xs` | `replaced` counts `&mut` of a container with owning elements as a replacement, unless it is a leaf | the same, for the binding: each use of `s` in the body |

Not a correctness problem: the drop walks the elements and frees into the region, which is
cheap.

**Where the code is.**

| Piece | Where |
|---|---|
| What is done with an element `index_mut` lent | `Contents::lent_to_keep` in `dz_flow/src/release.rs` |
| `for e in &mut xs` refused unless the container is a leaf | `replaced` in the same file |
| The test that pins what is kept | `an_indexed_container_skips_its_drop_unless_an_element_is_replaced` (`kept_in_a_let`) in `dz_flow/tests/placement.rs` |

**What it would take.** Compiler work only (`docs/log/`).
