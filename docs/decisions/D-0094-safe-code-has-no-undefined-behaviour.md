---
id: D-0094
title: Safe code has no undefined behaviour: closing the holes ties leave
status: decided
---

**Decided: option A, all of it.** Points 1, 2 and 3 were decided as proposed on 2026-11-02,
with point 2 as option A (one place per `T`), and are built (r103). Points 4–8 were decided
as proposed on 2026-11-03 (r107); points 4 to 7 are built (r108, r109, r111, r112), and
point 8 is written (r113). Checking the holes before writing it found one more, decided the
same day: a static's drop at exit could read a static C++ had already destroyed, so every
static is now dropped in a fixed order and left at its default (T-0163). The audit of the
generated C++ (T-0133) found the mirror image on 2026-11-04: a static's field default could
run code before `main` that read a static C++ had not built. The user chose that such
statics are built at the start of `main` in a fixed order, the reverse of the drops, a
static read early being built where it is read (T-0166, r116). Point 6 as T-0135's options A and C
together (a header trusted once in the manifest, spelt `keeps = […]` in its recipe, and a function taking only values safe
without a word), point 7 as T-0136's option A (a `kills` clause may name a static). Each
task holds the program that shows its hole, to become a test that must be refused; §1.1 is
rewritten (T-0160) once the holes it promises are closed are closed.

**The question.** Should Dazzle promise that a program with no `unsafe` has no undefined
behaviour on one thread: no read or write of freed memory, no payload used under the wrong
variant, nothing C++ leaves undefined? That is Rust's promise. Dazzle would keep the
aliasing Rust refuses. If so, which rules close the holes?

**Why it is asked.** §1.1 lists memory safety as a non-goal, but D-0091 and D-0093 have
taken Dazzle most of the way. Use after move, double free, and a reference used after a call
moved its memory are compile-time errors. On 2026-11-01 a review on the branch
`t-0104-regions-prototype` wrote small safe programs to test the rest. The checker accepted
each program below, and each one read freed memory or reached C++ undefined behaviour when
it ran:

| Program | What happened | Task |
|---|---|---|
| `append_copy(&mut ys, &ys)`, a list appended to itself | 7 of 16 strings copied from freed memory, silently, in debug and release | [T-0123](../tasks/T-0123-call-may-reach-memory-it-kills.md) |
| `change(&mut s, &mut s)`: a union's variant replaced through one argument, the old payload written through the other | segfault | [T-0123](../tasks/T-0123-call-may-reach-memory-it-kills.md) |
| the first program through two `Rc`s to one list | the same corruption | [T-0124](../tasks/T-0124-two-rc-may-hold-one-value.md) |
| `if let &n = h.node { h.node = null … n … }` on a `Box` | freed memory read | [T-0125](../tasks/T-0125-box-emptied-while-if-let-holds.md) |
| `if let &mut n = slot.target` on a pointer field | a write through a dangling pointer, with no `unsafe` | [T-0126](../tasks/T-0126-if-let-turns-untied-pointer-into.md) |
| a view struct whose `drop` reads its view, after the text it views grew | freed memory read in the drop | [T-0127](../tasks/T-0127-view-struct-drop-not-counted-as.md) |
| `find(…) or unreachable` when the search fails | release: the optimiser ran the loop past the list's end | [T-0128](../tasks/T-0128-or-unreachable-undefined-behaviour-in-release.md) |
| `MIN / -1`, `10 / 0`, `1 << 40`, `1e20 as i32` | a CPU trap for the first; the rest are undefined in C++ | [T-0129](../tasks/T-0129-division-by-zero-min-1-oversized.md) |

What held: signed overflow (`-fwrapv`), bounds checks in every build, use after move,
iterator invalidation, views of temporaries, an empty infinite loop, `defer` (its code, not
§6.5's sentence, [T-0130](../tasks/T-0130-6-5-says-defer-runs-after.md)), and `as`, which
refuses to turn an integer into an enum.

**Why the holes can close without giving up aliasing.** Rust's rule is that memory is
shared or changed, never both at once. Dazzle's is weaker, and on one thread it is enough:
memory may be shared and changed, but never freed or given another variant while something
else can still see it. That holds exactly when the checker knows every alias of the memory a
kill ends. Each hole above is a place where it loses one: two arguments of a call (1), two
handles (2), a `Box` (3), a drop (T-0127). The rest are C++ undefined behaviour reached
through codegen (4, 5).

**The proposal.**

1. **A call may reach what it kills through one argument only**
   ([T-0123](../tasks/T-0123-call-may-reach-memory-it-kills.md)). A callee is checked
   once, for every caller, with its parameters taken as separate. So at a call, memory the
   clause kills may be reached through the argument the clause names it by, and no other.
   An argument reaches memory by pointing into it, or by pointing at something that owns
   it. `f(&mut a, &a)` stays legal when `f` kills nothing, and so does `&mut xs[i]` beside
   `&xs[j]`. Rust refuses both.
2. **Every `Rc<T>` and `Weak<T>` is one place per `T`** to the ties check
   ([T-0124](../tasks/T-0124-two-rc-may-hold-one-value.md)), since any two may share a
   value. Point 1 and the existing kill check then catch the second program at compile
   time, at no cost at run time. The alternative is a borrow flag checked at run time,
   as Rust's `RefCell` has.
3. **A `Box`'s value is in the `Box`** ([T-0125](../tasks/T-0125-box-emptied-while-if-let-holds.md)),
   revising D-0062's "a `Box` can dangle as a `^T` can". `h.node = null` then kills what
   `if let &n = h.node` bound, as assigning a `String` kills its text.
4. **`or unreachable` panics in every build** ([T-0128](../tasks/T-0128-or-unreachable-undefined-behaviour-in-release.md)),
   revising D-0020. The optimiser's assumption stays available, written
   `unsafe { x or unreachable }`.
5. **Arithmetic is defined** ([T-0129](../tasks/T-0129-division-by-zero-min-1-oversized.md)),
   extending D-0063. Integer `/` and `%` by zero panic, as an index out of bounds does.
   `MIN / -1` wraps to `MIN`, and `MIN % -1` is 0, by D-0063's rule. A shift count is
   taken modulo the width. A float converted to an integer saturates, and NaN becomes 0.
6. **A C or C++ call is trusted where its header is imported, not at each call**
   ([T-0135](../tasks/T-0135-every-c-c-call-trusted-without.md)), revising §15's "a call
   into a header is ordinary safe code". A module trusts a header in one line that can be
   found with `grep`. A function that keeps a pointer past the call is marked `unsafe` to
   call. This is Rust 2024's `safe fn` inside `unsafe extern`, with no word at each call.
7. **A `kills` clause may name a static** ([T-0136](../tasks/T-0136-writing-static-unsafe-so-mutable-global.md)),
   revising D-0046. A write to a static becomes safe when the clause says what it kills, so
   mutable global state is checked rather than `unsafe`.
8. **§1.1 is rewritten**: safe Dazzle has no undefined behaviour on one thread. The stated
   exceptions are `unsafe` blocks, `std`'s trusted bodies (checked in debug builds,
   T-0116), and threads ([T-0011](../tasks/T-0011-threads-mutex-atomics-job-system.md),
   v2).

**What needs no decision.** [T-0126](../tasks/T-0126-if-let-turns-untied-pointer-into.md)
and [T-0127](../tasks/T-0127-view-struct-drop-not-counted-as.md) enforce rules already
decided (D-0093 point 11, and §6.7's first sentence). Four are tests and compiler work:
[T-0131](../tasks/T-0131-memory-tracking-poisons-every-freed-block.md) (freed memory
poisoned under tracking), [T-0132](../tasks/T-0132-sanitizer-builds-in-ci-run-corpus.md)
(sanitizers in CI), [T-0133](../tasks/T-0133-audit-generated-c-for-undefined-behaviour.md)
(an audit of the generated C++) and
[T-0134](../tasks/T-0134-fuzz-ties-checker-against-sanitizer.md) (a fuzzer for the
checker). [T-0130](../tasks/T-0130-6-5-says-defer-runs-after.md) corrects §6.5's sentence
to what the compiler does.

**What it keeps.** Aliasing as §6.7 has it, no lifetime parameters, D-0003 and D-0093 as
they are. Every fix refuses only a program that can reach freed memory or another variant's
payload, or that C++ leaves undefined.

**The options.**

- **A. All of it** (proposed).
- **B. Points 1–5 and 8**: every hole closed, with C and C++ calls and statics left as the
  stated trust boundaries. Points 6 and 7 can follow on their own.
- **C. Neither**: §1.1 stays, and the holes are documented, as raw pointers are.

A or B means a §21 revision row for each point as it lands, revising §1.1, §6.5, §6.7, §5.1
and §9.4, §14.5, and for A §7.7 and §15 too.
