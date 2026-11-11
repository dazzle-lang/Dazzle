---
id: D-0097
title: Homes: ownership that travels with its memory
status: open
---

**Where it stands.** A design, worked out with the user on 2026-11-11 and agreed in
direction, drafted to be tried before it is decided. Nothing here is in the spec, and the
syntax is a sketch. It reaches the spec, with a §21 row, when the user decides it. It
answers [T-0150](../tasks/T-0150-value-stored-into-container-lent-function.md), and
meets [D-0096](D-0096-threads-safe-multithreading-built-on-ties.md) (threads) at piece 7.
**Agreed so far** and **still open** are listed at the end.

**The question.** The compiler chooses where a value's memory comes from: a region, or
the heap (§14.6, D-0091). `move` hands a value's ownership to another owner, but leaves its
memory where it was. When the new owner outlives that memory, the value dangles. How should
ownership travel, so that a transfer stays as cheap as a pointer steal and is never unsafe?

## Why it is asked

A string built by a function that pushes it into a list it was lent goes to the heap, even
when the list is in its caller's region (T-0150):

```dz
fn add(names: &mut List<String>, i: i32) kills names@items {
    names.push(format!("a fairly long name number {}", i))   // heap: passed to `names.push`
}
```

Building it with the list's own allocator, which the list already stores, would be right
for this caller and wrong for another, because an element can leave the list:

```dz
fn give(from: &mut List<String>, store: &mut Store) kills from@items, from[*], store.names@items {
    let s = from.pop() or return
    store.names.push(move(s))          // valid today: every element of a lent list is the heap's
}

fn lent(store: &mut Store) {
    let mut names = List<String>()     // region: `lent`
    add(&mut names, 1)                 // in the list's allocator: `lent`'s region
    give(&mut names, store)            // now `store` holds it after `lent` returns: freed
}
```

Each function passes its own check. `lent` sees only `give`'s signature, and `add` decides
for every caller in every module. So placement plays safe: a container lent as `&mut`
keeps the heap, for the values its own function pushes too, and today's rules for what a
container may be given rest on inference (which parameters a method stores, which
elements are taken back out).

The cause is that ownership and memory are separate things in Dazzle, and `move` knows only
the first. In C++ and Rust every owned allocation is the heap's, so moving ownership never
had to ask where the memory was. In Dazzle it does.

**How much code moves things.** Few programs write `move`: outside `std`, the examples
have three, two in the `ownership` demo. In `tests/`, most are `return move(x)`, a
rebinding (`let b = move(a)`), a consuming `for`, or a container's own `construct(…,
move(v))`. The shape that breaks regions, an element out of one container and into another,
appears twice, both in `tests/run/regions`, written to test it.

## The rule

**Every value that takes memory from an allocator has a home: the region it was placed in,
or the heap.** Ownership may travel only to an owner whose home dies no later than its own.

- **The home exists already at run time.** A `String`, a `List` or a `HashMap` stores the
  allocator it was placed in (`m_allocator`, read from `#placement`). What is new is that
  the checker knows it, as a tie: `x@home` (name open), the span the memory is valid for.
  It costs nothing at run time.
- **Inside one home, a transfer is a rename.** `let b = move(a)`, passing by value,
  returning, and pushing into a container whose home is the same or shorter are pointer
  steals, as now, and always safe.
- **A container's elements live in the container's home.** What is pushed is built there,
  and what comes back out (`pop`, `remove`, `swap_remove`, a consuming `for`) is still
  there: `pop(&mut self) -> T? in self@home`. §6.7 today lets that result go untied ("hands
  back the element, which points wherever it did"); that is the hole, and this closes it.
- **Homes are ordered by scope.** A region dies before the regions of the blocks around it,
  and the heap never dies. Inside a function, a local's region dies before anything a
  parameter owns. Two parameters' homes are unordered unless the signature says otherwise.
- **Leaving a home is a copy, or declared.** Putting a value into an owner that may outlive
  its home is an error. Either copy it, which builds a value of its own in the new owner's
  home with its cost visible, or say in the signature that it goes there (`sends`, below).
- **What owns no allocator memory has no home.** `File` (a handle), numbers, `Key<T>`, a
  struct of those: it may go anywhere, as now. `Rc`, `Weak` and statics are the heap's.

The result: **a transfer never needs a copy the program did not write**, because the
compiler places each value, at its birth, in a home that outlives everywhere it will go.

## `sends` clauses

A function that hands what one input owns to another says so, after the return type, as it
says what it kills:

```dz
fn give(from: &mut List<String>, store: &mut Store)
    kills from@items, from[*], store.names@items
    sends from[*] to store                         // sketch: spelling open

pub fn push(&mut self, v: T, loc: SourceLocation = #location)
    kills self@items
    sends v to self                                // std's, written by hand like its kills
```

- **It is checked both ways**, as a `kills` clause is (§6.7): the body may send nothing the
  clause leaves out, and `dz fix` writes a missing clause.
- **The caller places by it.** `lent` lends `names` to `give` beside `store`, so what
  `names` holds must outlive `store`'s home: the compiler places `names` in a home that does
  (the heap, when `store`'s is a parameter's), and the editor hint says why ("heap: `give`
  sends its elements to `store`"). A caller that lends `names` only to functions that send
  nothing out of it keeps it in its region, and every function it lends it to builds into
  that region.
- **It replaces inference.** Which parameters a method stores into `self` (`storing_params`
  in `dz_flow/src/storing.rs`) and whether an element is taken back out (`lets_go` in
  `dz_flow/src/placement.rs`) are read from signatures instead of from bodies, so they hold
  across modules and for `pub` functions, as `kills` clauses do. The user turned down a
  whole-module summary for T-0150; this is the same knowledge, written where a reader and a
  caller in another module can see it.

So the T-0150 case works: `add` sends nothing out of `names`, so it may build in `names`'
home, and every caller that keeps its list in a region gets the string there.

## Variants

- **A. Homes with `sends`.** As above. Proposed.
- **B. Homes without `sends`: leaving a home is always a copy.** One rule fewer, and the
  same as T-0150's "language rule on pop". The price is that `give` has to write
  `store.names.push(last.copy())`, a copy that would not be needed if the caller had placed
  `names` on the heap.
- **Rejected: decide at run time.** Compare the two homes when ownership moves, steal when
  the source outlives the destination, copy otherwise. It always works and needs no new
  rule, but the copy is hidden, which is what principle 1 (§1) rules out and what made the
  user want a new approach: a move should be light.

## How it meets threads (D-0096)

D-0096 piece 7 gives placement two rules of its own for threads: a value moved to another
thread goes to the heap, and so does a container lent `&mut` into a parallel call, since
two threads would bump one region. With homes, both are one rule: **a thread allocates only
in homes it owns**, its own region pool or the heap.

- Moving a value to another thread is a send to an owner that may outlive every region of
  the sender, so its home must be the heap. `spawn`'s parameters are `sends … to` the new
  thread, and the caller places the value there at its birth, with no copy.
- A container lent `&mut` to a worker is one the worker builds into (`add` above). Placed
  by the clause, it is in the heap when the call is parallel; a later design may give
  workers a home of their own to fill and hand back.
- A worker's result, built in its own pool, reaches the parent as a send between homes,
  checked rather than assumed to be the heap's.

This is why homes come before threads: D-0096's "sendable" is then a property of a value
and its home together, not only of its type.

## What it would change, once decided

- **§6.4**: what `move` means: a transfer of ownership within what the value's home allows.
  Whether the spelling stays `move` or becomes something else is open.
- **§6.7**: the home tie, the `sends` clause, and the result of `pop` and its kind no
  longer untied.
- **§14.2 and §14.6**: elements live in their container's home; the "lent as `&mut`, goes to
  the heap" and "taken back out, goes to the heap" rules are replaced; a function lent a
  container builds into its home.
- **`std`**: `sends` on every method that stores a parameter (`push`, `insert`, `append`,
  `replace`, `get_or_insert`, `SlotMap::insert`, `Box(…)`, …), written by hand and checked
  as its `kills` are.
- **The compiler**: the home tie in `dz_flow`'s ties check; the proposer in
  `dz_flow/src/placement.rs` reading `sends` instead of inferring. T-0150 closes;
  T-0145's and T-0149's inference is replaced.

## Before deciding: try to break it

As for D-0094 and D-0096, write the programs first: those that must be refused, with the
rule that refuses each, and those that must keep compiling.

1. **Refused**: `give` above with no `sends`; a popped element returned from a function
   whose result has no `in`; an element of a region list pushed into a `static`'s list; a
   `HashMap::replace`'s old value kept past the map's region; a value sent through a
   generic `fn f<T>(xs: &mut List<T>, out: &mut List<T>)` with no clause.
2. **Kept, with no copy**: a work queue that pops and drops; `lent` with `names` on the
   heap because of `give`'s clause; `return move(xs)`; a `Box` built in a region and moved
   to a local of the same block; `examples/breakout`'s `swap_remove` read and dropped.
3. **Checked for cost**: `tests/run/regions` and the `words` and `expr` benchmarks, with
   how many allocations move from the heap to a region.

## Agreed and open

| | |
|---|---|
| **Agreed, 2026-11-11** | ownership must travel with its memory; a transfer stays a pointer steal and never copies by itself; a whole-module summary is not the answer; what leaves a home is declared or copied, never decided at run time |
| **Open** | variant A (`sends`) or B (copy only); the spelling of the home tie (`@home`?) and of `sends … to`; whether `move` keeps its name; whether a parameter may declare its home outright (`from` lives as long as `store`) instead of a `sends` clause; how generic code that only moves a `T` writes its clause |
