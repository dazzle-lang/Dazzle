---
id: T-0270
title: A chain of fifteen additions overflows the checker's stack
milestone: none
status: open
kind: bug
area: checker
---

Found on 2026-11-26 while writing `tests/run/packs` (T-0269), and there before it: `dz check`
on a debug build overflows the main thread's stack on

```dz
fn s(x: i32) -> i32 {
    return x + x + x + x + x + x + x + x + x + x + x + x + x + x + x + x
}
```

Twelve additions in a row check; fifteen do not. The compiler crashes instead of reporting
anything, so the editor's analysis dies with it (invariant 1's spirit: analysis never bails).
The likely cause is the checker probing operands of a binary chain (`binary`, `literal_shaped`
in `crates/dz_check/src/check.rs`), each level with a large frame in a debug build. Not yet
checked on a release build, nor with Linux's larger main-thread stack.

The fix is either a smaller frame per level (the probe's state off the stack) or the chain
checked as a list rather than by recursion. Test: a `tests/errors` or `typed` case with a
chain of a hundred terms.

## Where the code is

| Piece | Location |
|---|---|
| Binary operators checked | `crates/dz_check/src/check.rs` (`binary`, `literal_shaped`, `probe`) |
