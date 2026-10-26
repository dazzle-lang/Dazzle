---
id: T-0105
title: Placement follows the value's destination, not a function parameter
milestone: none
status: open
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
