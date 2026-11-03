---
id: T-0128
title: `or unreachable` is undefined behaviour in a release build
milestone: none
status: done
done: r108, 2026-11-03
kind: design
area: codegen
---

Point 4 of [D-0094](../decisions/D-0094-safe-code-has-no-undefined-behaviour.md), decided
as proposed on 2026-11-03 (r107): panic in every build, the optimiser's assumption kept as
`unsafe { x or unreachable }`. It revises D-0020.

**Where the code is**

| Piece | Where |
|---|---|
| The emission: `::dz::unreachable(loc)` in debug, `__builtin_unreachable()` otherwise | `dz_cpp/src/lib.rs`, `hir::OrClause::Unreachable` (~5596) |
| The panic | `::dz::unreachable` in `dz_cpp/src/runtime.rs` |
| Whether the clause is in `unsafe` | `Emitter::unsafe_depth`, counted in `block_into` and set in `in_body` from `owner_is_unsafe` (`dz_cpp/src/lib.rs`) |
| The tests | `overflow_wraps_and_unreachable_is_assumed_only_in_unsafe`, `unreachable_reached_panics_in_release` in `dz/tests/run.rs` |
| The rule | D-0020; §9.4; §5.2's example, "checked in debug, free in release" |

**What it is.** In a release build, `x or unreachable` tells the optimiser that `x` never
fails. When it does fail, behaviour is undefined, in safe code:

```dz
fn find(xs: &List<i32>, v: i32) -> usize? {
    for i in 0..xs.len() {
        if xs[i] == v { return i }
    }
    return none
}

let i = find(&xs, 99) or unreachable    // the search fails
```

A debug build panics: "reached code the compiler proved unreachable". The release build
concluded that `find` must succeed, so its loop cannot end without a match, and ran past
the end of the list. The bounds check inside it happened to stop it: "index 1 is out of
bounds for a List of length 1". Without that check, anything could happen.

**Why it matters.** It is the one place where safe code asks for undefined behaviour by
name. Code written as "this cannot fail" is exactly the code whose assumptions change
later.

**Why it is this way.** D-0020 chose speed: `or unreachable` is the loud spelling of "this
cannot fail", and §1.1 promised no safety.

**What it would take.** Panic in every build, as debug does now. That is a predictable
branch, costing about what a bounds check does. The optimiser's assumption stays available
for a hot path, written `unsafe { x or unreachable }`, so it is marked like everything else
unchecked. The emitter must know whether the clause is inside `unsafe` (the HIR's
`unsafe_blocks`).

A language change: revises D-0020, §9.4 and §5.2's example, with a §21 row.
