---
id: T-0168
title: Elements whose indices are known apart are separate places
milestone: none
status: done
kind: feature
area: ownership
---

Asked for by the user on 2026-11-04, after the comparison with Rust's borrow checker
(the site's Rust page): the first of the refusals both languages make that could soundly be
accepted.

**Where the code is**

| Piece | Where |
|---|---|
| `Step::Element(Index)`, `Index`, and `covers` comparing indices | `crates/dz_flow/src/ties.rs` |
| Which element an index expression names | `index_key`, `index_origins_at`, and the `Index` trait call in `call_inner`, in `crates/dz_flow/src/ties/body.rs` |
| Forgetting an index when its local is bound again | `forget_index` (called by `bind`) in `body.rs`; `Place::forget_index` in `ties.rs` |
| Clauses and messages still say `[*]` | `tie_path_held` in `crates/dz_flow/src/ties/report.rs` (`Place::unkeyed`) |
| Tests | `tests/flow/element_indices.dz` (both directions), `tests/run/element_indices` (run under the sanitizers in CI) |
| The rule | spec §6.7, "Elements share their container's ties, unless their indices are known apart" |

**What it was.** The check named every element of a container as one place, `xs[*]`, so a
call that changed one element ended a view of any other. `names[i].push_str(names[i +
1].as_str())`, `names[0]` beside `names[1]`, and a view into `nest[0]` kept while `nest[1]`
grows were all refused, as Rust refuses them.

**Done** (2026-11-04, r117). An element indexed by a literal, or by a local that is not `mut`
plus or minus a literal, keeps that index in its place: `names[Element(Offset(i, 1))]`. Two
places are compared step by step, and two elements are apart only when both indices are
literals that differ, or offsets of the same local by different constants. Such a local
changes only when its binding runs again (the next pass of a loop, a `let` in a loop), and
`bind` then forgets every index it keyed, so a view taken with `i + 1` is `[*]` by the time
the next pass kills `names[i]`. Everything else compares as before: a `mut` local, two
different locals, a literal beside a local, an index computed any other way. A declared
tie pattern matches an indexed element and gives up its index, and a clause or a message
still names `[*]`. Raw-pointer indexing (`unsafe`, std's bodies) is not keyed.
