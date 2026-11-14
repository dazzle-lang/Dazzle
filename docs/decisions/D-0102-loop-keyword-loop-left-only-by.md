---
id: D-0102
title: "`loop` is a keyword: a loop left only by `break` or `return`"
status: decided
---

**`loop { … }` runs its body until a `break` leaves it or a `return` leaves the function,
and takes a label as `while` and `for` do: `outer: loop { … }`. A `loop` that no `break`
leaves never ends but by `return`, so nothing after it runs, and a function that promises a
value may end with it. `loop` is a keyword. Chosen by the user on 2026-11-14 (T-0224);
spec §2.7, §10.2, §20.**

```dz
fn next_token(&mut self) -> Token {
    loop {
        let c = self.peek()
        if c != ' ' {
            return self.token_at(c)
        }
        self.advance()
    }
}
```

Like `while`, it is a statement and has no value: a `break` carries none (D-0028).

**Why.** Dazzle had no way to write a loop left only by `break` or `return` other than
`while true`, and definite return does not treat `while true` as never ending, so a function
whose only way out was a `return` inside one was refused (`DZ0405`) and needed a `return`
after the loop that could never run. `while true` keeps its meaning: a `while` may always
end, whatever its condition.

**Later (0.3, the user's plan).** When `dz fix` is worked on, the compiler will warn on a
`while true` and suggest `loop`, which `dz fix` will rewrite.

**The options weighed.** Treat a `while` whose condition is the literal `true` as never
ending, as Java and C# do for definite assignment; add `loop`, as Rust has; or keep the rule
and say in the help that a `while true` needs a `return` after it. The user chose `loop`,
and to steer `while true` towards it later rather than give the literal a meaning of its own.
