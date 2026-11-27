---
id: T-0272
title: A call that kills through one argument while another is an Rc that may hold the same value
milestone: M19
status: done
kind: bug
area: ownership
---

**Fixed 2026-11-27**, the day it was found by the aliasing matrix (T-0274): the caller now
counts an argument's `Rc`s and `Weak`s as reaching what dies when it is in an `Rc`'s value
(DZ0420), as §14.5 already said it must; `tests/errors/kill_beside_an_rc_to_the_same_value.dz`.
The mirror, a kill through an `Rc` argument beside a view into its value, is refused inside the
function already (DZ0413).

**Found** by the aliasing matrix (T-0274). Safe code reads freed memory on one
thread, no `parallel` involved:

```dz
fn keep(a: &mut List<i32>, b: &Rc<List<i32>>) kills a@items {
    if let &l = b {
        let first = &l[0]
        for _ in 0..1000 {
            a.push(7)                  // may move the list `l` is
        }
        println!("first is {}", first)
    }
}

fn main() {
    let mut xs = List<i32>()
    xs.push(42)
    let mut rc = Rc<List<i32>>(xs)
    let rc2 = rc.clone()
    if let &mut v = rc {
        keep(&mut v, &rc2)             // accepted: `v` is the list `rc2` holds
    }
}
```

`dz run --track-memory` prints `first is -572662307`, the tracker's poison. `keep` is right for
every `a` apart from what `b` holds, and is checked so, with its parameters taken as separate;
its `kills` clause tells the caller to keep `a` apart from the other arguments (DZ0420). The
caller does, for a place, but `&rc2` is a handle, and what a handle reaches is every value an
`Rc<List<i32>>` holds (§14.5), the place `v` is in. The caller's check does not count an
argument's handles as reaching that place.

**The fix.** In the caller, an argument whose type reaches an `Rc<T>` or `Weak<T>` reaches the
place every `Rc<T>` stands for (`shared_root`, and `handles_reached` from T-0266), so a kill
through an argument in that place conflicts with it. The parallel conflict check (T-0266)
already counts handles this way; the kill check does not.

The matrix's cases: `kills/rc_value_and_*`, `kills_keeping_a_view/rc_value_and_*`,
`growing_block_in_function/rc_value_and_*`.

## Where the code is

| Piece | Location |
|---|---|
| The caller's check (DZ0420) | `crates/dz_flow/src/ties/body.rs`: `invoke` (`through_handle`, where it pushes `AliasedKill`), given `Handles` by `handles_of` from `call_inner`; `shared_type_of`; reported in `ties/report.rs` |
| Every `Rc<T>` as one place | `crates/dz_flow/src/ties/body.rs`: `shared_root`, `meets_shared`; `handles_reached`, `handles_meet` in `crates/dz_check/src/threads.rs` |
| The matrix | `crates/dz_driver/tests/aliasing.rs`, `KNOWN` |
