---
id: D-0097
title: Homes: ownership that travels with its memory
status: decided
---

**Decided: variant A, homes with `sends` clauses** (r119, 2026-11-11, by the user), after
the programs below were written and tried. The spellings are in **Decided** at the end.
Built under [T-0200](../tasks/T-0200-homes-built-sends-clauses-home-tie.md), and in the
spec: §6.4, §6.7, §14.2 and §14.6, with `sends` in §2.7 and §20. It answers
[T-0150](../tasks/T-0150-value-stored-into-container-lent-function.md), and meets
[D-0096](D-0096-threads-safe-multithreading-built-on-ties.md) (threads) at piece 7.

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
  the checker knows it, as a tie: `x@home`, the span the memory is valid for. It costs
  nothing at run time.
- **`@home` is reserved.** No struct declares it: a value's home follows from where it was
  placed, and its owned parts live where it lives. A struct that declares `tie @home` is an
  error, whose help suggests another name, as a keyword is taken from identifiers.
- **A value has one home, and each of its parts its own.** A value gets its home at birth,
  and its parts with it. A part assigned later may come from a home that lives longer (a
  list assigned to a field of a region's struct is the heap's), never shorter. A value's
  home is the shortest of its parts' homes, since that is how long all its memory is valid.
  Each part keeps its own, per path as every tie is: `store.saved@home` may be the heap
  while `store@home` is a region, so moving `store.saved` out is checked against the heap,
  and moving `store` whole against the region.
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
    sends from[*] to store

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

- **A. Homes with `sends`.** As above. **Chosen by the user, 2026-11-11.**
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

## What it changed (T-0200)

- **§6.4**: what `move` means: a transfer of ownership within what the value's home allows.
  The spelling stays `move`.
- **§6.7**: the home tie and its reservation, a value's home as the shortest of its parts',
  the `sends` clause, and `in x@home` on an owned result, so `pop` and its kind are no
  longer untied.
- **§14.2 and §14.6**: elements live in their container's home; the "lent as `&mut`, goes to
  the heap" and "taken back out, goes to the heap" rules are replaced; a function lent a
  container builds into its home.
- **`std`**: seventeen signatures as built (T-0200 stage 3): `sends` on the eight that store
  a parameter less `Box(…)` (a constructor's value holds what it is given), on
  `List::append` and on `HashMap`'s private `place`; `in self@home` on the six that hand
  an element back, `HashMap`'s private `remove_slot`, and `StrKeys::remove_str`. Checked
  as every body is, once in the test suite rather than in every build.
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

## Tried, 2026-11-11

The programs above, and more, are `tests/run/transfers`: each moves a value out of a
container whose memory could be a region, into an owner that outlives it, then reuses the
region and reads the value back. It runs with regions on and off and under the sanitizers.

**Today has no hole.** Every probe's value is placed on the heap, each with its reason in
`dz placements`: `append` through a `&mut` parameter, a popped element passed through a
function that returns it, `remove` and `swap_remove`, `HashMap::replace`'s and `remove`'s
values, into an optional field, a drain by `pop`, into a static, a generic mover, a `Box`, a
whole list moved out, an element returned out of a local list and out of a lent one, and the
same across modules through `pub` functions and a container of another module's. A result
function that returns a value it did not build (`return xs.pop() or String()`) is not
dropped by its caller's region either, so it does not leak. Two shapes need no probe,
because the language already refuses them: moving out through a reference (DZ0402) and a
consuming `for` (DZ0501).

So homes adds no safety the compiler lacks today. What they would add is placement, and
inference written down where another module can read it.

**What would move off the heap.** Of 118 heap sites across `tests/run` and the
workspaces, and none in the benchmarks (`words`, `expr` and `astar` have no heap site at all)
or the examples, four are the shape homes change, and all four are in `tests/run/regions`,
written to test it: `tag` and `captions` filling a list they are lent (T-0150), and `label`
and `lend_copy` pushing into a lent store's list. `tests/run/transfers` adds three more:
an inner block's element pushed into the outer block's list (today the heap, with homes the
outer block's region), an element of a local list returned (the caller's region), and
another module's `tag`. Those are programs of one module, a few hundred lines, with no
frame loop.

**A game's frame, measured.** A game has the shape homes is for, so it was measured as
one. The program had 64 units in a long-lived `World`. Each pass of the loop builds an
`Events`, a `DrawList` (both from an `engine` module) and a `List<List<Vec2>>` of paths.
Systems fill them through parameters: `combat` emits 16 events with messages too long to
keep in place, `plan` builds 8 paths of 12 points, `render` pushes a rect and a short label
per unit, and the engine's `draw_hud` adds two lines of text. `malloc` was counted per frame
by an `LD_PRELOAD` shim, and each call was traced back to its `.dz` line:

| Build | `malloc`s per frame | From |
|---|---|---|
| everything on the heap (`DZ_REGIONS=0`) | 45 | the frame's three containers growing (10), and everything below |
| regions, as today | 35 | 16 event messages (`events.emit`), 16 path buffers (`paths.push`), 2 HUD lines and 1 more `format!` (`draw.text`, inside the engine) |
| the same work written in the loop body | 16, now 0 | the 16 path buffers, until `stored_after_let` |

Today's regions take the containers off the heap but nothing put into them, because every
one of those values crosses a function or a module on the way in. Written in one body, where
the compiler sees every push, the events, labels and HUD text go to the frame's region, with
identical output. That is what homes would do across the boundaries. The 16 path buffers
left were a different gap, closed the same day: `let mut path = List<Vec2>()`, filled, then
`paths.push(move(path))` inside a loop kept the heap, because the proposer let a value built
on every pass into a region only when stored directly, and a `let` in between made it not
direct. A `let` whose local a later statement of its block moves into the container, with
no `break`, `continue` or reassignment between, now counts as direct
(`storing::stored_after_let`). Written in one body, the frame now makes **no `malloc`**,
and runs 10% faster than on the heap (0.54 s against 0.60 s).

The same runs, timed (200,000 frames, release, clang++, best of 7): regions were **12%
slower than the heap** (0.68 s against 0.61 s). Profiled, the cost was a list growing in a
region. `List` grows elements that are not moved as bytes (a `DrawCmd` holds a `String`)
by a fresh block, a move of each element and a free of the old block. A region gives
nothing back for a block that is not its last, so every doubling of the draw list left the
one before in the region until the frame ended. With the lists reserved up front, regions
were already 4% ahead. `Allocator::resize_in_place` (2026-11-11) grows a region's last
allocation where it is, moving nothing. With it, **regions are 11% faster than the heap** on
this loop (0.54 s against 0.61 s). Entering and leaving a region costs less than the
`malloc` and `free` it replaces: 10 million calls of a function that builds a small list
take 0.10 s in a region and 0.14 s on the heap, since the pool hands back the chunk with no
`malloc`.

**Built (T-0200 stage 4, 2026-11-11).** With placement reading `sends`, a function lent a
container builds into its home, the frame's systems fill the frame's lists in the frame's
region, and the loop makes **no `malloc` per frame**: 0.56 s against 0.61 s on the heap,
same output, clean under the sanitizers.

**What the inference costs across modules.** `Staying::stores`
(`dz_flow/src/placement.rs`) trusts what a method stores into `self` only for a caller in
the method's own module or a method of `std`. A container of another module's, such as
`movers::Shelf` in `tests/run/transfers`, gets the heap for every value it is given, wherever
it lives. A game whose engine module defines its containers loses every region there.
`sends` on `Shelf::put` would carry the fact across, as `kills` does. This is the stronger
case for variant A.

**Inside one body nothing is refused.** The compiler sees every move a body makes. When a
local's element goes to a parameter's container, a static, or another block, it places the
local in a home that outlives the destination at the local's birth, today's heap or a
region that lives long enough. So "an element of a region list pushed into a `static`'s
list" and "a `HashMap::replace`'s old value kept past the map's region" (item 1 above) are
refused only when the container is a parameter. Written with a local, they are placements.
Refusals happen only at a signature, and under either variant they are these:

| Program | Variant A | Variant B |
|---|---|---|
| a generic `move_last<T>(xs, out)`, popping from one and pushing into the other | needs `sends xs[*] to out` | needs `out.push(v.copy())` |
| `last_of(xs) -> String`, returning `xs.pop()` | needs its result tied to `xs@home` | needs `xs.pop().copy()`, or the same tie |
| `give` / `take_last`, from a lent list into a lent store | needs `sends from[*] to store` | needs a copy |
| an element of a lent list pushed into a static | needs a `sends` to the static | needs a copy |

**`std`'s clauses** are fifteen signatures. Eight store a parameter into `self`: `List::push`
and `insert`, `HashMap::insert`, `replace` and `get_or_insert`, `HashSet::insert`,
`SlotMap::insert`, and `Box(…)`. `List::append` moves between two containers
(`sends other[*] to self`). Six hand an element back and take the container's home on their
result: `List::pop`, `remove` and `swap_remove`, `HashMap::replace` and `remove`, and
`SlotMap::remove`. `Rc(…)` needs none, since it is always the heap's.

**New questions it raised.** The last three are added to **Open** below.

- **An owned result tied to a home.** `in` ties a *reference* today. A popped value returned
  out of a lent list is owned and lives in the list's home, and the caller must place by
  that rather than by its hidden result placement. Is that `-> String in xs@home`, the same
  `in`, or something of its own?
- **Sending to a static or the heap.** `sends xs[*] to KEPT`, or `to heap`, since every
  static's home is the heap.
- **A clause is vacuous for a `T` that owns no memory.** `move_last<i32>` sends nothing
  anywhere, but `move_last<T>` must carry the clause for every `T`, as `kills` does.

## Decided

All on 2026-11-11, by the user.

| | |
|---|---|
| **The direction** | ownership travels with its memory; a transfer stays a pointer steal and never copies by itself; a whole-module summary is not the answer; what leaves a home is declared or copied, never decided at run time |
| **Variant** | A, `sends` clauses, over B (copy only) |
| **The home tie** | `@home`, reserved: no struct may declare it. `@self` was weighed: it needs no reservation, since `self` is a keyword, but `self@self` reads badly and `store.names@self` reads as all of `store.names` |
| **Several homes** | a value has one home, the shortest of its parts'; a part's home is never shorter than its owner's; each part keeps its own, per path |
| **`sends`** | `sends from[*] to store`, after the `kills` clause; checked both ways, and written by `dz fix` |
| **A home on a parameter** | not for now: `sends` says what a function does, where a bound such as `from: &mut List<String> in store@home` would constrain the caller without saying why. It can come later if real code asks for it |
| **An owned result in a parameter's home** | `in`, as for a reference: `fn last_of(xs: &mut List<String>) -> String in xs@home`, and `std`'s `pop(&mut self) -> T? in self@home` |
| **A by-value parameter returned** | the same `in`, required (2026-11-11): `fn largest<T: Ord>(a: T, b: T) -> T in (a@home, b@home)`. Placing every argument whose type the result could hold where the result goes was weighed and turned down: it needs no clause, but it is a hidden guess that gives arguments which are never returned a longer home than they need, and "could hold" grows rules for tuples, boxes and containers. The clause says exactly which arguments travel, and the rest stay in the caller's region |
| **A send to a static** | the static by name, `sends xs[*] to KEPT`, as `kills` names one (r112). Whether `to heap` is needed too is left to D-0096, for `spawn` |
| **Generic code** | the clause is always written, `sends xs[*] to out` in `move_last<T>`, and checked as if `T` owns memory, as `kills` is; it says nothing for a `T` that owns none |
| **`move`** | keeps its name and its meaning: a pointer steal, now refused where it would outlive its memory |
