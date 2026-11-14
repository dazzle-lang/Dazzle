---
id: T-0227
title: A move on a path that ends in return is counted when a loop body runs again
milestone: none
status: open
kind: feature
area: flow
where: `crates/dz_flow/src/lib.rs`: `loop_body`
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
