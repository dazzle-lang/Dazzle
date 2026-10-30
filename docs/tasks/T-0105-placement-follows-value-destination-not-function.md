---
id: T-0105
title: Placement follows the value's destination, not a function parameter
milestone: none
status: done
done: r81, 2026-10-27
kind: design
area: ownership
where: `crates/dz_cpp/src/regions.rs` on the branch `t-0104-regions-prototype`
---

Stage 3 of the regions prototype ([T-0104](T-0104-prototype-compiler-chooses-where-memory-comes.md)),
and the model the user's questions on 2026-10-26 pointed to.

**What it is.** An allocator belongs to a *value*, not to a function call. Where a value's
memory should come from is decided by where the value ends up:

| The value ends up | Its memory comes from |
|---|---|
| a local that is only borrowed | the scope's region |
| returned | wherever the caller wants the result (the result allocator) |
| pushed or inserted into a container | that container's allocator |
| stored in a field of something long-lived | that owner's allocator, or the heap |

The case that shows why a function parameter cannot express this:

```dz
fn tag(names: &mut List<String>, prefix: str, n: i32) {
    let mut scratch = String::from(prefix)       // dies here: this scope's region
    scratch.push_str("-")
    names.push(String::from(scratch.as_str()))   // lives as long as `names`: its allocator
}
```

One `allocator: ^mut Allocator = #allocator` parameter used for both strings puts both in
the same place, which is wrong for one of them. Used for neither, stage 2 already gets the
scratch string right (a region) and the pushed one safe but not best (the heap).

**Why it matters.** The pushed string is the common case: `words` (T-0090) spends its memory
on `String` keys copied into a map, and stage 2 cannot reach them. Containers of strings,
lists of lists and maps of lists are what game code builds.

**Why it is this way.** Nothing tells the compiler that `list.push(s)` keeps `s` inside
`list`. It has to be read from the callee: `push` moves `v` into memory the list owns, but
through `unsafe { construct(m_data[i], v) }` on a raw pointer, not through a field
assignment the analysis could see. So the facts needed are:

- **per parameter, where it goes**: "`v` is stored inside `self`", "`v` is returned",
  "`v` only borrowed", computed from each function's body, including through `construct`,
  `move_out` and assignment through `self`'s own pointers (spec §14.3);
- **whose allocator `self` has**, at the call: the receiver's `m_allocator`, which is a run
  time value, so the argument's construction would be given `list.allocator()` (a method the
  containers would need) rather than a region decided at compile time;
- the summaries are whole-program, so a change to one function's body can change the code
  generated for its callers. That is acceptable in `dz build`, which already works on the
  whole program for monomorphization, and invisible to the LSP, since no diagnostic depends
  on it.

**What it would take.** Compiler work on the prototype branch, no language change by
itself: the per-parameter summaries (with `std`'s containers written by hand first, to learn
whether inferring them is worth it), a way to ask a container for its allocator, and the
`tag` case above as a test that prints the same either way. Then re-measure `words`.
Whether the language keeps `#allocator` parameters at all is
[T-0106](T-0106-keep-manual-allocators-only-at-construction.md).

**Prototyped on 2026-10-27** on the same branch, behind `DZ_REGIONS=1` like stages 1 and 2.

The hole the table above misses, found first: **a container can give an element back.**
`list.pop()`, `remove`, `swap_remove` and `HashMap::replace` move an element out, and it
can then go anywhere — into a field of something long-lived, out of the function. If the
element's memory came from the list's region, it dangles when the region ends. So "pushed
into a container: that container's allocator" holds only for a container that **keeps**
what it is given until it is dropped. `tests/run/regions` shows it: with the check switched
off, `take_last` pops a string `tag` pushed into a region list, keeps it in a store that
outlives the block, and the store then reads `������` from a chunk `churn` reused.

What the prototype does:

- **Which calls store an argument** is a table in `regions.rs` (`STORES`), written by hand:
  `List::push` and `insert`, `HashMap::insert` and `get_or_insert`, `HashSet::insert`. A
  value built there by a call that leaves its allocator to the compiler — `String::from(…)`,
  `List<T>()`, a function with a result allocator — is built with the container's
  allocator for its contents; a `String` passed by copy is copied there (`String::copy_in`,
  since a copy otherwise takes the source's allocator).
- **Whose allocator, at the call**: `List`, `HashMap` and `HashSet` gained
  `contents_allocator(fallback)`, which asks the container's allocator (`Allocator::contents`,
  new, `null` by default). The answer is the region only for a container the compiler built
  as one that keeps; every other one answers `fallback`, the value's own default, so a
  container given `Allocators::Temp` or an arena by hand behaves exactly as before.
- **Which containers keep** (`keeps` in `regions.rs`): a `List`, `HashMap` or `HashSet` of
  something that owns memory, placed in a region by stage 1, whose every use is a `std`
  method that neither takes an element out nor drops one (`TAKES`, plus any method
  returning something that needs a drop), a borrow, an index, a `for`, or a `&mut` lent to
  a function whose parameter keeps by the same rules. That last part is the **per-parameter
  summary, inferred** from the function's body, once for the whole program; `tag_twice`
  passing its parameter on to `tag` is followed. Assigning to the container, popping,
  clearing or lending it to a method of a user type all say no. Such a container is built
  with a `ScopeShared` view of the block's region rather than the `ScopeArena` itself, and
  only the view says the region may hold its contents.
- **Only one level.** What an element itself stores goes back to its own default
  (`ScopeShared::contents` returns the arena, whose own answer is `null`): an element of a
  `List<List<String>>` can be popped from even when the outer list keeps.
- `ScopeArena::free` now gives back the last small allocation, so a key `HashMap::insert`
  already had, built just before and dropped at once, costs the region nothing.

Not done: a user type's own storing methods (`self.items.push(v)` inside `add(v)`) are not
summarised, so `store.add(String::from(x))` keeps the heap; a value bound by `let` first and
moved in (`let s = …; list.push(move(s))`) and a struct literal's fields (`list.push(Item{
name: String::from(x) })`) are not followed; the explanation is `DZ_REGIONS_EXPLAIN` lines
(`into`, and `region … and what it stores`), not inlay hints.

Tests: `tests/run/regions` gained `tag`, `tag_twice`, `take_last` and `distinct` (keys copied
into a map, as in `words`), and prints the same with regions on and off on clang++ and g++,
with no C++ warnings. The whole `tests/run` corpus passes with regions off (clang++), on
(clang++) and on (g++), apart from the `cmake` program and the `sdl3` example, which need
Ninja on this machine, as before. Building the prototype also turned up a codegen bug that
would bite outside it: a trait method naming the trait's own object type (`contents()
-> ^mut Allocator`) emitted a vtable that named `Allocator__dyn` before it was declared. The
handle is now forward-declared ahead of every vtable.

The A/B, Windows, release, ten runs each, alternating, peak working set; *hand*, *heap* and
*regions* as in T-0104:

| program | compiler | hand | heap | regions | peak MiB (hand / heap / regions) |
|---|---|---:|---:|---:|---|
| words | clang++ | 376 ms | 400 ms | 394 ms (−2%) | 73 / 46 / 51 |
| words | g++ | 331 ms | 354 ms | 348 ms (−2%) | 73 / 46 / 51 |
| astar | clang++ | 703 ms | 699 ms | 616 ms (−12%) | 22 / 22 / 29 |
| astar | g++ | 564 ms | 563 ms | 485 ms (−14%) | 22 / 22 / 29 |

(`expr` is left out: its `Parser` takes an allocator with no default, so the heap version is
not the same edit, and its memory is `Box` nodes, which no stage places.)

What it says:

- **It reaches `words`' keys** (`DZ_REGIONS_EXPLAIN` says `into … a copy: wherever .insert
  keeps it`, and the map is built with the shared view), and the peak is near the heap's:
  51 MiB against 46, where stage 2 was 60. But the time gained is 2%, and hand-placed `Temp`
  is still 6–7% ahead. The keys were not where `words` spends its time; what `Temp` buys it
  is everything else — the 2 MB `text` grown by doubling, the `word` buffer, `entries` and
  the copies of the keys made into them, which go through a `for` binding and a struct
  literal that stage 3 does not follow.
- **`astar` is as it was** (−12 to −14%, against −15 to −17% for stage 2 on the same
  machine, within this machine's noise), so asking a container for its allocator at every
  storing call costs nothing that shows.
- **Against the go/no-go in T-0104:** the analysis stays explainable — one rule, "a container
  that never gives anything back" — and memory stays close to the heap's, but the wins
  remain where the program's own functions build and drop whole containers. Copying a key
  out into a struct is the next thing `words` would need, which means following values
  through `let`, `for` bindings and struct literals, not more summaries.

**Superseded as a direction on 2026-10-27** by [D-0091](../decisions/D-0091-ties-checked-reference-invalidation-memory-placed.md)
and its design, [T-0107](T-0107-design-ties-kills-clauses-compiler-placed.md): a checker that knows
when memory dies makes placement safe by construction, and the hand-written `STORES` and
`TAKES` tables here are what it would replace.

**Done, 2026-10-27:** prototyped, and superseded by D-0091 (r81); T-0107 replaces the hand-written tables with `kills` clauses.

**Removed, 2026-10-30 (r90):** the code described here left the branch, and is kept at the git tag `regions-prototype` (commit `548b771`).
