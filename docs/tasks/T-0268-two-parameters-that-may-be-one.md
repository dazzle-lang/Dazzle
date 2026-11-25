---
id: T-0268
title: Two parameters that may be one place reach a parallel block or loop
milestone: M19
status: open
kind: design
area: concurrency
---

**Found 2026-11-25**, building `parallel for` (D-0120). A data race that safe code can write
today, through the `parallel { … }` block as much as the loop.

**The hole.** D-0094 lets a `&mut` and a `&` to the same value reach one function, `f(&mut a,
&a)`, when `f` kills nothing, and the ties analysis checks a function once with its parameters
taken as separate places. A block or a `parallel for` inside such a function runs code writing
through one parameter beside code reading through the other, and nothing refuses it:

```dz
fn fill(xs: &mut List<i32>) {
    xs[0] += 1
}

fn total(xs: &List<i32>, out: &mut i64) {
    for &x in xs {
        out += x as i64
    }
}

fn both(a: &mut List<i32>, b: &List<i32>, out: &mut i64) lends a {
    parallel {
        fill(a)             // writes what `a` reaches
        total(b, out)       // reads what `b` reaches, at the same time
    }
}

fn main() {
    let mut xs = List<i32>()
    xs.push(0)
    let mut out: i64 = 0
    both(&mut xs, &xs, &mut out)    // accepted: `a` and `b` are one list
}
```

The same with `parallel for &mut x in a { … b[0] … }`. A call that kills is caught already
(DZ0420), since a `kills` clause makes the caller keep that argument apart from the others;
a write that kills nothing has no such clause. Inside one body the loop now refuses reading
the container its passes write (DZ0433), and the block's calls are held to each other
(DZ0424); only parameters, which the body cannot see the caller's arguments for, slip by.

**Options, the user's call.**

1. **The caller keeps such an argument apart**, as for `kills`: a function whose block or
   parallel loop writes through a parameter says so in its signature, and its callers may not
   pass anything else that reaches the same place. `lends p` says nearly this already, but only
   where what is written may grow; it could cover every write, or a second word could.
2. **The body refuses it**: inside a function, a block or loop that writes through one
   parameter may not read another whose type could reach the same memory. Simple, but it
   refuses `step(ps: &mut List<P>, cfg: &Config)` only if `Config` could hold a `List<P>`.
3. **A stated exception** in §22.1, as §1.1 once named threads: the weakest.

## Where the code is

| Piece | Location |
|---|---|
| The block's conflict check, places per body | `crates/dz_flow/src/ties/body.rs`: `record_parallel`, `parallel_conflicts` |
| What a body lends, and the clause | `crates/dz_flow/src/ties/body.rs`: `lend_to_thread`, `record_lends`; `report.rs`: `report_lends` |
| The caller keeping a killing argument apart (DZ0420) | `crates/dz_flow/src/ties/report.rs`, `codes::ALIASED_KILL` |
| The loop's own check (DZ0433) | `crates/dz_check/src/check/parallel_for.rs`: `reads_written` |
