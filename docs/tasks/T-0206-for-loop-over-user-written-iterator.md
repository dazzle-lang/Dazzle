---
id: T-0206
title: A for loop over a user-written iterator checks nothing: for _, &mut h in query(set)
milestone: none
status: open
kind: bug
area: ownership
---

Found on 2026-11-12 while writing the sparse-set ECS example (`examples/ecs`). Safe code,
no `unsafe`, writes freed memory: a hole in §1.1's promise and in §6.7.

**What it is.** Inside a `for` over a user-written `Iterate` view struct, nothing is
followed: neither a use of the loop's bindings after a kill, nor the loop's next pass over
memory its body killed. It compiles clean:

```dz
view struct One<T> {
    pub(module) pool: &mut List<T>
    row: usize = 0
    tie @items = pool
}
fn one<T>(pool: &mut List<T>) -> One<T> { return One<T>{ pool: pool } }
impl<T> Iterate for One<T> {
    type Item = &mut T
    fn valid(&self) -> bool { return row < pool.len() }
    fn get(&self) -> &mut T in self@items { return &mut pool[row] }
    fn advance(&mut self) { row += 1 }
}

fn grow(xs: &mut List<i32>) kills xs@items {
    for &mut x in one(xs) {
        xs.push(1)        // wrong: no DZ0413 here (the loop goes on over killed memory)
        x += 1            // wrong: no DZ0413 here (`x` points into `xs@items`)
    }
}
```

The same loop over the container, `for &mut x in xs` or `xs.iter_mut()`, gives both
`DZ0413`s. It does not depend on the head: a call (`one(xs)`), a local (`let it = one(xs)`
then `for … in it`), single bindings and tuple bindings (`for _, &mut x in two(xs)`, or
`for item in two(xs)` destructured in the body) are all missed. The explicit cursor is
checked: `let q = one(xs)`, `xs.push(1)`, `q.get()` is a `DZ0413`. So only the loop is
blind.

In the ECS: `for _, &mut h in query(&mut world.health) { world.health.insert(…); h.hp -= 1 }`
compiles.

**Why it happens.** In the `For` arm of `Cx::stmt`, a subject that is not a container's
`iter()`/`iter_mut()` (`iterated_container` is `None`) takes its elements from
`index_origins`, as if the iterator were indexed. A view struct has no `Index`, so that
records `unchecked("the indexed element's storage is unknown")` and returns no origins: the
bindings point nowhere the check follows. The iterator itself is `self.targets(subject, …)`,
the call's result or the local holding it, not what it views, so a kill of the viewed
memory does not end it either. A tuple shape adds `unchecked("tuple iterator bindings are
not modeled yet")`. `unchecked` notes are read only by `dz_flow`'s own tests, so the user
is never told.

T-0107's census listed "general iterators" as still open, and T-0111 (done) says that
item "goes with it", but the loop was never connected to `Iterate::get`.

**What it would take.** Follow the loop as the explicit cursor already is: the iterator's
origins are what the view struct views (`what_it_views`, D-0093), and each pass binds the
result of `Iterate::get` through the implementation's signature (`in self@items` through a
view field, `through_view_field`), with a tuple `Item` giving each part its own origins
(`Value::parts`). The next-pass check then works as for containers (`InvalidUse::next_pass`).

Tests: a `tests/flow` file with the `One` loop above, a tuple-item iterator, a loop over a
local iterator, and a `break` right after the kill staying clean; a `dz_flow/tests/ties.rs`
case pinning that no `unchecked` note is left for these loops; and `examples/ecs` still
checking clean.

## Where the code is

| What | Where |
|---|---|
| The `for` loop | the `hir::StmtKind::For` arm of `Cx::stmt`, `crates/dz_flow/src/ties/body.rs` |
| Container loops (followed) | `iterated_container`, `iterated_origins` in the same file |
| Where the elements are lost | `index_origins_at` (the `index_signature` lookup that fails) |
| What a view struct views | `what_it_views`, `through_view_field` |
| The unreported notes | `unchecked`, `Result::unchecked` |
| The container loop's tests | `tests/flow/loop_kills_its_container.dz` |
