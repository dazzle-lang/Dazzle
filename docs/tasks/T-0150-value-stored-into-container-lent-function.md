---
id: T-0150
title: A value stored into a container lent to a function keeps the heap
milestone: none
status: open
kind: design
area: ownership
---

Split out of [T-0149](T-0149-stored-values-left-on-heap-map.md), its second row, which was
not built because the answer it proposed is not sound on its own.

**Where the code is**

| Piece | Where |
|---|---|
| The proposer's target for a stored value, which needs the container to be a `let` of the body | `stored_target`, `receiver_local`, `stored_by` in `dz_flow/src/placement.rs` |
| What the check carries out of a container: only what this body stored into it | `taken_out`, `region_store`, `Regions::stored` in `dz_flow/src/ties/body.rs` |
| Where a container keeps its allocator | `m_allocator` in `std/collections/list.dz`, `hash_map.dz` |

**What it is.** A value stored into a container the function was lent is built on the heap,
wherever the container lives:

```dz
fn tag(names: &mut List<String>) kills names@items {
    names.push(String::from("x"))      // heap: the caller's list, wherever it is
}
```

**Why it matters.** Filling a container a caller owns is a common shape (a parser appending
to a list it was given, `captions(&mut caps, n)` in `tests/run/regions`), and each such
value is a `malloc` the caller's region could have served.

**Why it is this way.** T-0149 proposed building the value with the container's own
allocator, which it stores (the prototype's `contents_allocator`, T-0105). That places the
value wherever the container is, so it is right only while no element leaves the container
for somewhere that outlives the container's region. Nothing guarantees that today, and the
check cannot see it:

```dz
fn give(from: &mut List<String>, store: &mut Store) kills from@items, from[*], store.names@items {
    let s = from.pop() or return
    store.names.push(move(s))          // valid today: every element of `from` is the heap's
}

fn lent(store: &mut Store) -> usize kills store.names@items {
    let mut names = List<String>()     // region: `lent`
    tag(&mut names)                    // with T-0149's answer, "x" is in `lent`'s region
    give(&mut names, store)            // and now in `store`, after `lent` returns: freed
    return names.len()
}
```

`give` is checked alone and is right by itself. `lent` sees only `give`'s signature. The
string's region is decided in `tag`, for every caller, in any module. This is the prototype's
stage-3 bug: an element popped out of a region container and kept elsewhere read freed
memory.

**What it would take**, one of:

- **A rule in the language** that an element taken out of a container lives no longer than
  the container's memory, unless it is copied, as a tie on what `pop` and `remove` return.
  It would make `give` above an error. It changes what valid programs are, so it is the
  user's call (§18).
- **A summary per function, within a module** as `Staying` is: which `&mut` container
  parameters it only adds to and never lets an element leave. A callee could then build in
  the container's allocator only when every caller is in its module and lends it only to
  such functions, and the caller's check would treat elements taken out of a container it
  lent as carrying the container's own region. Compiler work, but whole-module, and nothing
  for a `pub` function called from elsewhere (T-0142).
