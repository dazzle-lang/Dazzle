---
id: T-0227
title: A move on a path that ends in return is counted when a loop body runs again
milestone: none
status: done
kind: bug
area: flow
where: `dz_flow/src/lib.rs`: `loop_body`, `LoopFrame`, and the `Break`/`Continue` arm of `read`; `tests/flow/loop_*`
---

Found writing the `json` benchmark's parser (T-0173, 2026-11-14). A loop body whose last
statement returns a value moved out of a local, with a `continue` above it, was reported as
moving the local twice (DZ0401, "this runs more than once"), and its earlier use as a use
after the move:

```dz
fn array(&mut self) !-> Json {
    let mut items = List<Json>()
    loop {
        let v = self.value() or return
        items.push(move(v))                      // reported: used after the move below
        if self.at(',') {
            self.skip()
            continue
        }
        return Json::Array{ items: move(items) } // reported: runs more than once
    }
}
```

The `return` leaves the function, so the body never runs again after it moves `items`.
`loop_body` checks a body once quietly, then again from the entry state joined with what
the first pass left moved, and that state is the block's end, which here is reached only
through the `return`. It should be the join of the states at each `continue` that restarts
this loop, and at the block's end only when the end is reachable. Using the entry state
whenever the body diverges would be simpler and wrong: a move before a `continue` would go
unseen, and a real double move with it.

The benchmark's parser avoids it with a condition variable instead of `continue`. A test
for each half: the program above accepted, and `loop { take(move(x)); if c { continue };
return }` still refused.

**Done** (2026-11-17, r140), filed as a bug: a valid program was refused. And the same
fault hid one the other way, which was worse. A `break` diverges like a `continue`, so what was
moved before it never reached what follows the loop: `if c { take(s); break }` and then a
use of `s` after the loop was accepted.

Each loop being walked now has a frame (`LoopFrame`), and a `break` or `continue` records the
moved set into the loop it names, the innermost or the one its label picks. The second pass
of a body starts from the entry state, joined with the body's end only where the end is
reachable, and with each `continue`. After the loop, the state is the start of a pass, where a
`while` or `for` may stop, joined with each `continue` and `break`. Tests:
`tests/flow/loop_return_ends_the_last_pass.dz` (accepted),
`loop_move_before_continue.dz` and `loop_move_before_break.dz` (`DZ0401`).
