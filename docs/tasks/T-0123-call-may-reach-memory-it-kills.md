---
id: T-0123
title: A call may reach memory it kills through only one argument
milestone: none
status: open
kind: design
area: ownership
---

Point 1 of [D-0094](../decisions/D-0094-safe-code-has-no-undefined-behaviour.md), which is
open: the rule is the user's call.

**Where the code is**

| Piece | Where |
|---|---|
| A call's arguments (`receiver`, `params`), its clause's kills bound to them, and the check that refuses an argument the call kills | `dz_flow/src/ties/body.rs`, the `kills` loop at the end of the call walk (line ~1084, "Independent argument aliases…") |
| Whether a kill ends a view | `Origin::ended_by` and `Reach` in `dz_flow/src/ties/body.rs`; `Place::covers` in `dz_flow/src/ties.rs` |
| The diagnostics | `dz_flow/src/ties/report.rs`; a new code beside `DZ0413` in `dz_diag/src/codes.rs` |

**What it is.** A function is checked once, for every caller, with its parameters taken as
separate places. Inside `append_copy` below, `dst` and `src` are two lists. A caller may
still pass one list as both. The call check refuses an argument that points *into* memory
the call kills (`think(&mut world.enemies[i], &mut world)`, §6.7). It does not refuse one
that points at the *owner* of that memory, as `&ys` does:

```dz
fn append_copy(dst: &mut List<String>, src: &List<String>) kills dst@items {
    for &s in src {
        dst.push(String::from(s.as_str()))   // the first push moves the array `src` walks
    }
}

let mut ys = List<String>()     // filled to its capacity: eight strings
append_copy(&mut ys, &ys)       // accepted: 7 of the 8 copies are read from freed memory
```

The same through a union is a segfault. Below, `b` replaces the variant and then the old
payload is written through `n`, which `a` bound. The clause is the one the compiler asks
for:

```dz
fn change(a: &mut Shape, b: &mut Shape) kills b, a {
    switch a {
        case Named(&mut n) {
            b = Shape::Square{ side: 1 }    // drops the `String` that `n.name` is
            n.name.push_str("!")            // writes the dropped string
        }
        case Square { }
    }
}

change(&mut s, &mut s)                      // accepted; segfaults
```

`apart(a, b)` in `tests/flow/use_after_kill.dz` relies on the same assumption inside the
callee, which is right there. T-0116's debug-build guard catches `apart(&mut xs, &mut xs)`
when the function returns, after the stale read has happened. It does not see `&ys`, which
is not a `&mut`.

**The rule proposed.** At a call, memory the clause kills may be reached through the
argument the clause names it by, and through no other argument. An argument reaches memory
by pointing into it, or by pointing at a place that owns it. Two arguments that both name a
kill of the same memory are refused too, since the callee takes them for two.

```dz
append_copy(&mut ys, &ys)            // error: `ys@items` dies through `dst`, and `src` reaches it
append_copy(&mut ys, &zs)            // fine
rebuild(&mut world, &world.config)   // error when `rebuild` kills `world.config`: its text dies
rebuild(&mut world, &world.score)    // fine: an `i32` owns nothing that can die
f(&mut a, &a)                        // fine when `f` kills nothing
look(&mut xs[i], &xs[j])             // fine: nothing dies
```

**Why it matters.** It is the hole most likely to appear in real code. Appending a list to
itself, merging a container into itself, and passing an object along with one of its own
fields are all common. The result is silent corruption, or a crash far from the cause.

**Why it is this way.** §6.7 keeps aliasing, and the call check was written for references
*into* killed memory. A reference to the owner was taken as safe, since the owner itself
does not move.

**What it would take.**

- In the `kills` loop, also refuse an origin whose place is above the kill
  (`origin.target.covers(&kill)`), or one under it that owns memory the kill reaches. The
  argument that names the kill is exempt.
- Apply this to the receiver as to any parameter, and to a view struct's views
  (`what_it_views`). By-value parameters stay exempt (`holds`), since they are copied
  before the call.
- Run the corpus and the examples, and count what the rule refuses there that is not a
  bug. That count measures how much freedom the rule costs.
- The placement check walks the same ties (`analyse_regions`), so run
  `dz_flow/tests/placement.rs` too.

A language change: a §21 row and a new diagnostic code.
