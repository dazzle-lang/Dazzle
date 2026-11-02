---
id: T-0145
title: A value stored into a container is built where the container lives
milestone: none
status: open
kind: feature
area: ownership
---

Found by [T-0119](T-0119-remove-allocators-from-ordinary-code.md), whose measurement it is
half of: what hand-placed `Allocators::Temp` gave `words`, and placement does not.

**Where the code is**

| Piece | Where |
|---|---|
| What becomes of a value passed by value ("passed to `counts.insert`") | `fate` in `dz_flow/src/placement.rs` |
| The summary of what a parameter does, within a module | `Staying`, `staying_params` in `dz_flow/src/placement.rs`; `stays_in_call` in `dz_flow/src/ties/body.rs` |
| The region check, which would follow an element taken back out | `analyse_regions`, `sink`, `passed` in `dz_flow/src/ties/body.rs` |
| The containers that store what they are given | `List::push`, `HashMap::insert` (`std/collections/`) |
| A copy, which is always on the heap today | [T-0140](T-0140-copy-built-on-heap-wherever-it.md) |

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
value stored into it is built where it lives, the drop can be skipped too: `releasable` in
`dz_flow/src/release.rs` would accept such a container when nothing stored into it came from
anywhere but its region.
