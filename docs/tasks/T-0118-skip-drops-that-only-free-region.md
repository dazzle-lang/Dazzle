---
id: T-0118
title: Skip drops that only free region memory
milestone: none
status: done
kind: feature
area: ownership
---

Step 5 of D-0091's plan, split out of [T-0107](T-0107-design-ties-kills-clauses-compiler-placed.md);
after [T-0117](T-0117-memory-placed-by-compiler-regions-result.md).

**Where the code is.**

| Piece | Where |
|---|---|
| Whether a drop only frees, per type instance: `Drops::only_frees`, the walk over a drop body (`Frees`; `fold` answers `T::has_drop` and the like per instance), `Box`'s function value (`boxed_only_frees`) | `dz_flow/src/release.rs` |
| Whether everything a value owns lives where it was built: `Drops::releasable` (`leaf`, a container; `composite`, a struct of them; `sealed`, no method replaces a part) | `dz_flow/src/release.rs` |
| Which functions build their whole value in their placement: `whole_functions` (`reads_placement`, `ctor_builds_fields`, `returns_whole`) | `dz_flow/src/release.rs` |
| Which locals skip: `skipped_drops` (`built_in_region`, `replaced`), run at the end of `place_with` | `dz_flow/src/release.rs`, `dz_flow/src/placement.rs` |
| What is recorded: `BodyPlacement::skips_drop`, `Site::drop_skipped`, `Site::label` ("…; drop skipped"), `Placements::mark_skipped` | `dz_flow/src/placement.rs` |
| Codegen: the local built in storage C++ never destroys, `_dz_keptN`, the name a reference to it | `stmt_inner`'s `Let`, `dz_cpp/src/lib.rs` |
| Shown: the inlay hint and `dz placements` read `Site::label`; `--format=json` has `drop_skipped` | `dz_lsp/src/ide.rs`, `crates/dz/src/main.rs` |
| `HashMap`'s drop visits its slots only when a key or value owns something, so it only frees | `dz_project/src/std/collections/hash_map.dz` |
| Tests | `a_drop_only_frees_when_its_body_does_nothing_but_free` and `a_drop_is_skipped_only_when_its_region_frees_everything` in `dz_flow/tests/placement.rs`; `inlay_hints_show_where_each_allocation_goes` in `dz_lsp/tests/ide.rs`; `dz_placements_prints_each_site` and `regions_change_nothing` in `dz/tests/run.rs` |

**What it is.** When everything a value owns is in a region that is about to be released, its
`drop` need not run if all it does is free memory: a `String` or a `List<i32>` in a scope
region costs nothing at scope exit. A drop with another effect (`File` closing a handle)
still runs.

**Why it matters.** Most of a region's speed comes from releasing it at once; walking every
value to free memory the region frees anyway gives part of it back.

**Why it is this way.** It needs placement first, and a way to know a drop only frees memory.

**What it would take.** Compiler work only (`docs/log/`).

**Settled in r90: no syntax; the compiler proves it.** A drop only frees when its body does
nothing but free memory the value owns, through `Allocator::free`, and drop parts whose own
drops only free. It is worked out per type instance, with monomorphization, since `List<T>`
depends on `T`: `List<String>` and `Box<String>` qualify; `File` (it closes a handle), `Rc`
(a count other owners see) and any drop that calls C, a function value or another trait
object do not. A marker a type wrote would be trusted, and wrong the day its drop gains an
effect. The placement hint says when a drop is skipped.

**Built 2026-11-02.** Behind `DZ_REGIONS`, as all placement is.

- **A drop that only frees**, per type instance: the drop body with the instance's types
  put in, walked for anything but reading, arithmetic, writing a local or a field of the
  value being dropped, a count `for i in 0..n`, `Allocator::free`, and `destroy` of a part
  whose drop only frees. An `if` made only of type questions, `!T::is_trivially_copyable`, is
  answered for the instance, so the branch it never takes is not followed. Every r90 example
  holds (`a_drop_only_frees_when_its_body_does_nothing_but_free`). `Box`'s drop calls a
  function value, `m_drop`; that one is followed, since it is the drop of what the box was
  made as, `T` or a type deriving from `T`, and all of those must only free.
  `HashMap`'s drop called `clear`, which writes every slot's hash before the arrays are
  freed; it now destroys entries only when a key or value owns something, so a map of plain
  values is freed without a pass over its slots, regions or not.
- **The second half, which r90 left open: is everything the value owns in the region?**
  Placement says where a value was built, not where everything in it came from. A
  `List<String>` in a region holds strings built on the heap (they are passed to `push`), and
  a `String` in a region may be replaced by one built elsewhere. Skipping either drop would
  leak, not corrupt memory, but a leak is still a changed program. So a local's drop is
  skipped only when all of these hold:
  - **built whole in its region**: `let x = site` placed in its block's region, or a struct
    literal whose owning fields are such sites, where the site's function builds everything
    the value owns in its placement: a container's constructor that stores `#placement`, a
    struct's constructor that sets each owning field once at the top to such a site, or a
    function that returns only such sites or a local bound to one;
  - **owns nothing from elsewhere**: its type is a container whose elements own nothing
    (`List<i32>`, `HashMap<i32, i32>`, `List<u8>`), or a struct with no drop of its own whose
    owning fields are such values and whose own methods never replace them (`String`, `Path`,
    `MinHeap<Node>`, a `Bag` of a list and a string);
  - **never replaced**: no assignment to it or to an owning part of it, no `&mut` of it handed
    to anything but its own methods and a `for` over its elements, no `construct` into it.
- **Codegen** builds such a local in storage C++ never destroys, and binds the name as a
  reference to it, so every use reads as before:
  `alignas(T) unsigned char _dz_kept1[sizeof(T)]; T& xs = *::new (…) T(…);`. The region is
  declared first in the block, so it is released after everything in the block.
- **What it reaches today**: in `tests/run/regions`, 15 of the 38 sites in a region; in the
  benchmarks, `astar`'s queue, both its maps and its path, and `words`' text and buffer.
  Kept: a container of owning elements (done since by T-0145, round 2), and the four cases
  [T-0147](T-0147-skip-more-drops-value-lent-as.md) lists — a value lent as `&mut`, one built
  by a trait method, a function value or `format!`, and a local in a generic body.
- **Checked**: every `tests/run` program with regions on and `--track-memory` (no leaks, the
  same output as with regions off), `regions_change_nothing` with the region C++ warning-clean
  on clang++ and g++, the `dz_flow`, `dz_lsp`, `dz_driver`, `dz_cpp` and `dz_project` tests.
  Not timed: the machine was busy, and the user set benchmarks aside until placement is
  finished (T-0117).
