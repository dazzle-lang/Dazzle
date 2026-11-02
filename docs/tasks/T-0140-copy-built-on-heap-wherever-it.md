---
id: T-0140
title: A copy is built on the heap wherever it is placed
milestone: none
status: done
kind: feature
area: ownership
---

Split out of [T-0117](T-0117-memory-placed-by-compiler-regions-result.md), stage 2,
round 3 ("`copy` is not done"), which left it for after the gate.

**Where the code is** (as built, 2026-11-02)

| Piece | Where |
|---|---|
| Which types copy into a placement, and how: `placed_copy_kind` (`PlacedCopy::Written`, `Memberwise`), `placed_copy` | `dz_flow/src/placement.rs` |
| A `copy` takes a placement, as a constructor does: `result_functions`; what it assigns to a field of `self` goes there (`propose`) | `dz_flow/src/placement.rs` |
| Copy sites: `copied` (in `place_body`), `Cx::copied_by_value`, followed as values of their own (`fate_as`, `temporary`); named "a copy of `x`" | `dz_flow/src/placement.rs` |
| The check: `passed` gives a copy site its region, `sink` refuses one that escapes, `Regions::seen` refuses one the walk never reached (in `analyse_regions`); `region_assign`'s `into_self` for a `copy` | `dz_flow/src/ties/body.rs` |
| A copy is never "built here" for a skipped drop: `built_in_region`, `built_here`; `whole_functions` leaves `copy` out | `dz_flow/src/release.rs` |
| The copies in std: `List::copy`, `HashMap::copy` build in `#placement` (`String`, `Path`, `HashSet`, `SlotMap` copy field by field through them) | `std/collections/list.dz`, `std/collections/hash_map.dz` |
| C++: `::dz::copy_tag` | `dz_cpp/src/runtime.rs` |
| C++: the placed copy constructor, declared in `struct_decl`, defined in `struct_bodies` (a written `copy`, with the plain copy constructor delegating with the heap) and `memberwise_copy`; `Emitter::placed_copy`; a copy site in `expr`; `struct_literal` is not an aggregate for such a type | `dz_cpp/src/lib.rs` |
| Tests | `a_copy_is_built_where_it_is_placed`, `which_copies_can_be_placed` and the copies in `printed` and `kept` in `dz_flow/tests/placement.rs`; `tallied`, `named_copy`, `named_of` and `lend_copy` in `tests/run/regions` |

**What it was.** A copy of a value that owns memory was always built on the heap, even when
the copy could live in its scope's region:

```dz
let names = other.names     // a copy of a `List<String>`: on the heap, even when `names`
                            // is only borrowed and could live in this scope's region
```

It mattered for speed only, since copies are correct on the heap (stage 1, step 3): a
`String` or `List` copied in a loop body is heap traffic a region could remove, and it was
where `words` spends its allocations (one key per distinct word, a copy of its buffer).

**Built 2026-11-02.** Behind `DZ_REGIONS`, as all placement is; with it unset the C++ is as
before but for the one bug below.

- **A copy is a site** where it is taken by value somewhere both steps follow a value: a
  `let`'s value, an argument, a part of a struct or union literal, a tuple or an array, what
  is returned (not a local, which C++ moves), and in a `copy`, what is assigned to a field of
  `self`. It is decided as any site is (`counts.insert(word, 1)` is stored into `counts`;
  `return self.names` is the result), following the copy's own value rather than the place
  it reads.
- **Which types**: a struct written in the program or `std`, with no base and no `virtual`
  method, not a `view struct`, whose fields keep an allocator, copyable at that instance,
  and either with a `copy` of its own (`List`, `HashMap`), which then takes a placement as a
  constructor does and reads it as `#placement`, or with neither `copy` nor `drop`, copied
  field by field with each field that keeps an allocator placed too (`String`, `Path`, a
  program's `Entry { word: String, … }`). Generics are monomorphised, so each instance gets
  the constructor exactly when `placed_copy` holds for it.
- **C++**: `T(::dz::copy_tag, const T&, Allocator__dyn _dz_result)` beside the copy
  constructor, which delegates to it with the heap, so every copy C++ makes on its own
  (copy assignment, a container's element copies) stays on the heap. A placed copy site is
  `T(::dz::copy_tag{}, x, placement)`. A type copied field by field spells out its special
  members, and is no longer an aggregate for `struct_literal`'s C++20 form.
- **The check** trusts that a copy builds only in its own placement, as it trusted the heap
  copy before. A copy site carries its own region and none of the original's; one passed
  where the walk does not follow it is refused; and one the walk never reached is refused
  rather than trusted.
- **Its drop is kept.** A copy is not "built whole in the region" for T-0118, since a
  `List<String>`'s elements are still copied by C++ on the heap; so a map whose keys are
  placed copies still runs its drop, which frees into the region.
- **std**: `List::copy` and `HashMap::copy` take `#placement` instead of the original's
  allocator, which they sent to the heap when it was a region (`Allocator::scoped`, kept for
  `ScopeArena`). With regions off, `#placement` in a `copy` is the heap, as the guard made it.
- **Found on the way**: a struct with a `copy` of its own and no `drop` did not compile,
  regions on or off. Its header defaulted the move constructor and move assignment, while
  `struct_bodies` defined them, as §6.4 asks, so a moved-from value is reset. The header
  now declares them for such a type too. No program had one until `tests/run/regions`'
  `Named`.
- **What it reaches**: `words`' keys, `counts.insert(word, 1)` under its `get_mut_str`
  guard (T-0149), and the words it copies into its `entries`, are in `main`'s region: 7 of
  its 8 allocations.
