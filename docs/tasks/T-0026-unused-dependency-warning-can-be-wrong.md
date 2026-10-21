---
id: T-0026
title: An unused-dependency warning can be wrong for a dependency used only through `.`
milestone: M9
status: open
kind: deferral
area: resolve
where: `dz_resolve`, the unused-dependency warning
---

**What goes wrong.** `dz` warns about a dependency in `dazzle.toml` that nothing uses. The
warning is computed in **name resolution**, which marks a dependency used when a `::` path
reaches into it. But invariant 9 draws the line precisely here: **`::` is resolved at compile
time, `.` is not** — a `.method()` needs the receiver's type and belongs to the type checker.

So a dependency whose only contribution is a **trait method**, called with `.` on a value
whose type comes from somewhere else, is never marked used:

```dz
# dazzle.toml names `pretty`, which declares `trait Show` and implements it for `f32`
let s = value.show()        # resolved by the checker; `pretty` is not marked used
```

The dependency is real — remove it and the program stops compiling — and the warning says to
remove it.

**Why it matters.** A warning that is sometimes wrong is worse than no warning, because it
trains people to ignore the ones that are right. The workarounds are to name the trait
somewhere (`use`-like, by writing a `::` path to it) or to ignore the warning, and neither is
something a user should have to know.

**What it would take.** The checker has the answer — it resolved the method, so it knows
which `impl` in which module provided it. The fix is for the usage marking to be **a product
of the checker as well as the resolver**: resolution marks `::` uses, checking marks method
and operator uses, and the warning is computed after both from the union. The shape to be
careful about is invariant 3 — every phase a pure function of its input — so the checker
should *report* used dependencies as part of its output rather than mutate the resolver's
state.

While there, the same hole almost certainly applies to anything else resolved by type rather
than by path: operator overloads (`a + b` provided by a dependency's `impl Add`), a trait
default method, and an `impl Drop` that makes a type move-only. Worth enumerating those and
testing each, since they fail the same way and a fix that catches only methods would leave
the warning still wrong.

A `tests/workspace/typed/` fixture per case: a dependency used only through `.`, only through
an operator, and genuinely unused, asserting a warning on the third and none on the first
two.
