---
id: T-0145
title: A value stored into a container is built where the container lives
milestone: none
status: done
kind: feature
area: ownership
---

Found by [T-0119](T-0119-remove-allocators-from-ordinary-code.md), whose measurement it is
half of: what hand-placed `Allocators::Temp` gave `words`, and placement does not.

**Where the code is** (as built, 2026-11-02)

| Piece | Where |
|---|---|
| Which by-value parameters a method stores into `self`, and which it keeps on every call: `storing_params` (`Walk::stored`, `moved_into_self`, `unconditional`), read through `Staying::stores`, `param_stored`, `param_kept` | `dz_flow/src/storing.rs`, `dz_flow/src/placement.rs` |
| Which containers keep everything stored into them (the loop rule's exception): `keeps`, `keeps_through` | `dz_flow/src/storing.rs` |
| The proposer: `Escape::Into`, `stored_by` (in `fate`), `stored_target`, `lets_go` (an element taken back out, the container lent as `&mut`), `receiver_local`, `loop_between` | `dz_flow/src/placement.rs` |
| The region check: `stored_into`, `region_store` (in `call`), `taken_out` (in `call`, around `call_inner`), `lend` (in `Ref`), `kept_each_pass` (in `region_site`), `Regions::stored` | `dz_flow/src/ties/body.rs` |
| A `List<String>` filled from its region skips its drop: `Drops::holder`, `Contents::whole_inside`, `built_here`, `foreign_adders`, `writes_through` | `dz_flow/src/release.rs` |
| A copy, which is always on the heap today | [T-0140](T-0140-copy-built-on-heap-wherever-it.md) |
| Tests | `std_containers_store_what_they_are_given`, `a_value_stored_into_a_container_is_built_where_it_lives`, `a_drop_is_skipped_only_when_its_region_frees_everything` and the corpus test in `dz_flow/tests/placement.rs`; `filled`, `popped`, `drained` and `counted` in `tests/run/regions` |

**What it is.** A value handed by value to a method that stores it in its container goes to
the heap, even when the container is in a region and nothing takes the value back out:

```dz
let mut counts = HashMap<String, i32>()    // region: `main`
let mut word = String()                    // region: `main`
counts.insert(word, 1)                     // the key, a copy of `word`: heap
names.push(String::from("x"))              // heap: passed to `names.push`
```

**Why it matters.** It is where `words` spends its allocations: one key per distinct word.
With the keys in `Temp`, as the program put them by hand before r104, it was faster than
the heap; T-0119 has the numbers. Any program that fills a local container with owned values
(a list of names, a map of strings) pays the same.

**Why it is this way.** Placement follows a value to where it ends up only through what
holds it whole (a `let`, a struct literal, a `Box`) and into a parameter that stays in the
call. A parameter stored into `self` is neither. The prototype's stage 3 (T-0105) placed
such values with their container and gained 2% on `words`, but its safety rested on a rule
found by a test: an element popped out of a region container and kept elsewhere read freed
memory.

**What it would take.** A summary per function, within a module as `Staying` is, that a
parameter is stored into `self` (std's containers among them); the proposer placing such an
argument where the receiver is placed, when the receiver is; and the region check treating
an element taken back out (`pop`, `remove`, a consuming `for`) as carrying the container's
region, so a wrong placement fails it rather than a test. For `words`' keys, which are
copies, T-0140's placed copy as well. Compiler work (`docs/log/`).

**It also widens T-0118 (2026-11-02).** A container in a region whose elements own memory,
`List<String>`, still runs its drop, since what was pushed was built elsewhere. Once every
value stored into it is built where it lives, the drop can be skipped too, when nothing
stored into it came from anywhere but its region. Done in round 2 below (`Drops::holder`
and `Contents` in `dz_flow/src/release.rs`).

**Built 2026-11-02, in two rounds.** Behind `DZ_REGIONS`, as all placement is.

- **Round 1: placed where the container lives.** Which parameters a method stores into
  `self` is inferred from its body: each use of the parameter borrows it, copies it, drops
  it, or moves it into `self` (`construct` into memory reached from a field of `self`, an
  assignment to a field, or a stored parameter of a method called on `self` or a field of
  it). So `HashMap::insert` is followed through `replace` and `place`, and `HashSet::insert`
  through its map. A parameter is **kept** when that move runs on every call: `List::push`,
  `List::insert` and `SlotMap::insert` keep; `HashMap::insert` and `get_or_insert`, which may
  drop the value they replace or were given, do not. `std`'s summaries are relied on from
  any module, a program's own within its module, as `Staying` is.
- **The proposer** places a value passed to such a parameter where the receiver lives, when
  the receiver is a local of the body (or a by-value field of one): its `let`'s block, or
  the result placement when the container is returned. It does not when an element taken
  back out (`pop`, `remove`) goes anywhere the region cannot follow, or the container is
  lent as `&mut` ("heap: passed to `store.names.push`, once taken out of `names`").
- **The loop rule's exception.** A value built on every pass of a loop the container's
  block encloses may go into its region only when it is stored at once, by a method that
  keeps it, into a container nothing in the body takes from, drops from or lends out
  (`keeps`). The region then grows no faster than the container. Otherwise it is "heap:
  stored in `names` on every pass of a loop".
- **The region check** follows the value into the container (its region joins the
  container's local, which must be declared inside the region's block) and back out: a
  `&mut self` method returning something that owns memory hands back what was stored, and
  `&mut` taken of the container loses it. It shares the loop exception's rule with the
  proposer, since that rule bounds memory and is not a safety rule. The corpus test, where
  the check refuses nothing the proposer placed, caught the proposer placing a string that
  was popped and kept elsewhere; `lets_go` is the fix.
- **Round 2: T-0118 for a container of owning elements.** A `List<String>` (`HashMap<i32,
  String>`, ...) skips its drop when everything ever in it was built whole in its region:
  every `&mut self` method called on it is its type's own, adds nothing it was not given
  (`resize`, which builds default elements, and `Extend::extend`, which copies, do), returns
  nothing that can write an element (`xs[i] =`, `get_or_insert`, `iter_mut`), and is given
  a site built whole in the region, directly or moved from a `let`. A local passed without
  `move` is a copy, built on the heap: the leak check found that one, in
  `tests/run/collections`' `HashMap<List<i32>, const &str>`.
- **What it reaches**: in `tests/run/regions`, the strings `main` pushes on every pass and
  `bags.push(Bag(2))`, and the new `filled` and `counted`, whose containers skip their drops.
  Not yet in the benchmarks: `words`' keys are copies (T-0140), and `expr`'s names are
  inserted into a map in a loop. Those and the other cases left are
  [T-0149](T-0149-stored-values-left-on-heap-map.md).
- **Checked**: every `tests/run` program with regions on and `--track-memory` (no leaks,
  the same output as with regions off), `regions_change_nothing` (the region C++
  warning-clean on clang++ and g++), the crates' tests, clippy and fmt.
