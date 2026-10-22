---
id: T-0057
title: A spread cannot be formatted
milestone: M12
status: done
done: r69, 2026-10-22
kind: deferral
area: checker
where: `dz_check/src/check.rs`, `format_macro`
---

**What is refused.** A spread argument is not an invocation a formatting macro accepts:

```dz
format!("{} {}", ..xs)      // refused
format!("{} {}", xs[0], xs[1])   // write them out
```

**Why.** The whole point of §12.9 is that the format string is checked **where it is
written**: the number of placeholders is compared against the number of arguments, and each
placeholder's spec against its argument's type, at compile time. A spread's length is a
*runtime* property — `..xs` holds however many elements `xs` has — so at the point
`format_macro` checks the string there is no count to compare and no per-argument type to
check a spec against. Accepting it would mean either giving up the count check for that
invocation, or deciding the count from the string and truncating or padding at run time,
which is the `printf` mismatch the design exists to rule out.

**Why it matters.** Rarely, and the workaround is usually what you wanted anyway: a variable
number of values is a loop, not a format string.

```dz
for x in xs { print!("{} ", x) }
println!()
```

What this does cost is a wrapper macro that wants to forward whatever it was given. §12.9's
own `log` example shows the supported way — a repetition in the *macro's* pattern
(`$( , $args: [,]Arg )?`), expanded into the inner invocation, so the arguments are written
out at expansion time and checked as the caller wrote them. That covers forwarding; it is
only a runtime-length collection that cannot be formatted.

**What it would take, if anything.** Probably nothing, and the row's value is in saying so:
the refusal is a consequence of the design rather than an unfinished piece of it. The
worthwhile work is the **diagnostic** — today this reads as the macro not matching, which
leaves the user guessing at the syntax; it should name the spread and say that a format
string is checked against arguments written out, with the loop and the repetition-in-a-macro
forms as the two answers. One `tests/errors` file pins it.
