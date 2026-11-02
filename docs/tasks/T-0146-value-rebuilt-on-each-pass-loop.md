---
id: T-0146
title: A value rebuilt on each pass of a loop, holding the one before, keeps the heap
milestone: none
status: open
kind: feature
area: ownership
---

Found by [T-0119](T-0119-remove-allocators-from-ordinary-code.md), whose measurement it is
half of: what hand-placed `Allocators::Temp` gave `expr`, and placement does not.

**Where the code is**

| Piece | Where |
|---|---|
| The proposer's loop rule, an assignment's value ("assigned to `lhs` on every pass of a loop") | `propose` (`StmtKind::Assign`) in `dz_flow/src/placement.rs` |
| A tuple `let` ("moved into a tuple") | `fate` (`StmtKind::LetTuple`) in `dz_flow/src/placement.rs` |
| The check's loop rule | `region_site`, `Refusal::EveryPass` in `dz_flow/src/ties/body.rs` |
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
