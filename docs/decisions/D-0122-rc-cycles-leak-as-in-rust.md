---
id: D-0122
title: "Rc cycles leak, as in Rust: safe, and named in a debug build"
status: decided
---

**Decided, 2026-11-28, r161: spec §1.1 and §14.5.** The user's choice, after asking what Rust
does and seeing what each option would refuse.

**The question.** Two values whose `Rc`s point at each other are never dropped: each keeps
the other's count above zero. The user's first aim was no leaks outside `unsafe`, with no
run-time overhead. With no cost at run time and no leaks, a correct program has to be refused
somewhere, so one of the three must give.

**The options.**

- **Refuse at compile time.** Refuse writing an `Rc` that could lead back into a value
  already reached through a shared `Rc`; that is the only way a cycle forms. It is sound and
  free at run time, but it refuses correct code that appends to a shared node in place. For
  example, a queue whose `tail` is an `Rc` refuses `tail.next = node`, though `node` is new
  and no cycle can form. The other cost is building it: following such writes through
  calls needs an effect per function, as `kills` is.
- **Collect cycles at run time**, as CPython does. Every program is accepted, but each `Rc`
  write and drop pays for it, and drops are no longer deterministic.
- **Leak, as Rust does.** A cycle leaks. That is not undefined behaviour: no value is read
  after it was freed, and nothing is freed twice. `Weak` is the way to write a back link.

**Decided: the third, with a debug build naming the cycle.** Leaking is safe, so §1.1's
promise, no undefined behaviour without `unsafe`, holds as it stands. §14.5 says so in plain
words, so nobody reads "safe" as "leak-free". A build with `track_memory` already lists every
leak at exit. Now it also names the `Rc` values that keep each other alive, each by the line
that made it, and says to make one link a `Weak`. Release builds pay nothing. A cycle shows
the first time a debug build runs, which is close to what a compile-time warning would give,
without refusing correct code.

**How the report works.** `Rc(…)` marks its block in the tracker (`track_rc_block`). At exit,
the words of each leaked `Rc` block are read as pointers, as `check_trusted`'s `reach`
already does. A leaked `Rc` block that reaches itself that way is on a cycle, and the ones
that reach each other are reported together. A `Weak` back link inside a structure that leaked
for another reason counts as a link too; the report says the values point at each other,
which is still true.
