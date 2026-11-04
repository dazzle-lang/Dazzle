---
id: T-0146
title: A value rebuilt on each pass of a loop, holding the one before, keeps the heap
milestone: none
status: done
kind: feature
area: ownership
---

Found by [T-0119](T-0119-remove-allocators-from-ordinary-code.md), whose measurement it is
half of: what hand-placed `Allocators::Temp` gave `expr`, and placement does not.

**Where the code is**

| Piece | Where |
|---|---|
| An assignment's value and a tuple `let` followed: `Escape::Held`, `held_target`, `kept_in`, `escape_of_all`, `held_by_itself` | `propose` and `fate` in `dz_flow/src/placement.rs` |
| What holds a part, shared by both steps | `holds_part` in `dz_flow/src/placement.rs` |
| A local that grows from itself | `grown_by`, `grows_itself` in `dz_flow/src/storing.rs` |
| The check's loop rule and its exception | `region_site`, `grown_each_pass` in `dz_flow/src/ties/body.rs` |
| The test | `a_value_that_holds_the_one_it_replaces_keeps_its_region` in `dz_flow/tests/placement.rs` |
| The program | `Parser::expr`, `term` and `main` in `tests/bench_programs/expr/src/main.dz` |

**What it is.** A value assigned on every pass of a loop goes to the heap, even when it
holds the one it replaces, so nothing made on a pass dies before the loop ends:

```dz
let mut lhs = self.term() or return
while true {
    let op = self.peek_operator('+' as u8, '-' as u8, '-' as u8) or break
    m_at += 1
    let rhs = self.term() or return
    lhs = Expr::Binary{ op: op, lhs: Box<Expr>(move(lhs)), rhs: Box<Expr>(move(rhs)) }
}                                          // each Box: heap, assigned on every pass
return move(lhs)
```

And `let (name, tree) = parser.statement() or …` keeps the heap too, since a tuple `let`
is not followed: "moved into a tuple".

**Why it matters.** It is where `expr` spends its allocations: every node of every parse
tree. Before r104 the program put each statement's tree in `Temp` by hand, released before
the next, and was 5–11% faster than the heap (T-0104); T-0119 has the numbers since.
Recursive-descent parsers, and any loop that grows one value from the last (`acc = f(acc,
x)` over owned values), are built this way.

**Why it is this way.** The loop rule (T-0117) sends to the heap a value made on every pass
while its region lives on, since a value that dies each pass would grow the region without
bound, which is where region inference failed before (the ML Kit). A value that moves the
one before it into itself does not die: the region grows with what is live, as the heap
would.

**What it would take.** The proposer accepting an assignment in a loop whose old value is
moved into the new one, and the region check agreeing, as a value that holds what it
replaces rather than a new one each pass; and a tuple `let` followed as a `let` of each
part. Then `statement`'s tree would be built in the region of the statement loop's pass,
which is what hand placement did. Compiler work (`docs/log/`).

**Done** (2026-11-04). A value assigned to a local of the body is placed where that local
lives, and a tuple `let` is followed as a `let` of each part. On every pass of a loop the
local outlives, an assigned value goes there only when the local grows from itself: every
assignment holds its old value through `move`, and nothing else drops it or gives it away.
The check agrees when the local is declared outside every loop the region outlives. None of
`expr`'s 21 allocations is on the heap now: 6 are in a region and 15 where the caller
keeps the result, and each statement's tree is in the region of the statement loop's pass. `expr` built before and
after, fifteen alternating runs: 221 → 211 ms on clang++, 216 → 204 ms on g++, about 5%.
Peak memory is unchanged. Spec §14.6 has the rule; `docs/log/r81-ties.md` the detail.
