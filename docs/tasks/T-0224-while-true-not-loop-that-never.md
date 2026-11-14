---
id: T-0224
title: "while true is not a loop that never falls out the bottom: definite return refuses it"
milestone: none
status: done
kind: design
area: flow
where: `crates/dz_flow/` (definite return, DZ0405)
---

Found writing the `json` benchmark (T-0173, 2026-11-14). **A language rule, so the user's
decision.**

Dazzle has no `loop` keyword, so a loop left only by `return` or `break` is written
`while true`. Definite return does not treat that condition as always true, so a function
whose only way out is a `return` inside it is refused:

```dz
fn collect(n: i32) -> List<i32> {
    let mut items = List<i32>()
    let mut i = 0
    while true {
        items.push(i)
        i += 1
        if i == n {
            return move(items)
        }
    }
}
// error[DZ0405]: this function must return `List<i32>` and does not on every path
```

The workaround is a loop with a condition variable, which is what the benchmark's parser
does. In the parser's first form the same shape also brought DZ0401 reports for a `move`
inside the `return` ("this runs more than once"), which did not reproduce on its own; they
most likely followed from the loop being taken to fall out the bottom, and should be
checked again with whatever is decided here.

**The choices**: treat a `while` whose condition is the literal `true` as never ending
(Java and C# do this for definite assignment; Rust has `loop` for it); add `loop { }`; or
keep the rule and say in the diagnostic's help that a `while true` needs a `return` after
it.

**Done 2026-11-14 (r131, D-0102).** The user chose `loop`, and to have the compiler warn on
`while true` and suggest `loop` later, in 0.3, with `dz fix` (T-0226). `loop` is a keyword;
the parser makes it a `while` with a `true` condition and `is_loop`, so every phase that
walks a `while` walks it unchanged; the checker marks each loop a `break` leaves, labelled
or not, and a `loop` none leaves is `forever` in the HIR, which definite return treats as
never falling through. The DZ0401 reports seen in the parser's first form come from
`loop_body` replaying a body from the moves of every path, those ending in `return`
included; telling them apart needs the state at each `continue`, and is T-0227.
