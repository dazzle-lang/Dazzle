---
id: T-0268
title: Two parameters that may be one place reach a parallel block or loop
milestone: M19
status: done
kind: design
area: concurrency
---

**Done 2026-11-27 (r158), by option 1, the user's call:** asked for an example, the user
read `lends a` as already meaning that `a` could not be given again. Now it does. A caller
gives a lent parameter nothing else that reaches what it does (DZ0434), the `Rc` case
included, and `lends` names every parameter parallel code writes, an `i32` too (DZ0425), not
only what may grow. Placement is unchanged: a caller puts on the heap only what the function's
`kills` says it may grow. The aliasing matrix's twelve cases are off its `KNOWN`;
`tests/flow/lends_keeps_apart.dz` and `tests/errors/lent_argument_reached_by_another.dz`.

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

**Through an `Rc` too** (found 2026-11-27, after T-0266): a parameter may be the value inside
an `Rc<T>`, and another parameter an `Rc<T>` to it, so `both(b: &mut Bag, r: &Rc<Bag>)` writing
through `b` in one call while another reads through `r` is the same race. And `lends a` does
not yet keep anything apart: the example above is accepted with it, and with `a: &mut i64`,
which grows nothing, no `lends` is asked for at all. The aliasing matrix
(`crates/dz_driver/tests/aliasing.rs`, T-0274) lists every case under `block_in_function` and
`loop_in_function`.

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
| The caller keeping a lent argument apart (DZ0434) | `crates/dz_flow/src/ties/body.rs`: `lent_apart`, called from `call_inner` after `invoke`; `LentAlias`; reported in `ties/report.rs`, `codes::LENT_ARGUMENT_REACHED` |
| What must be in `lends` | `crates/dz_flow/src/ties/body.rs`: `written_on_thread` (from `record_parallel`, the `For` arm, `record_lends`), `BodyAnalysis::written_on_threads`; `report_lends` in `report.rs` reads it with `lent` |
| The loop's own check (DZ0433) | `crates/dz_check/src/check/parallel_for.rs`: `reads_written` |
