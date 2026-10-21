---
id: T-0032
title: A name an expansion resolves where the macro is declared must be `pub` there
milestone: M10
status: open
kind: deferral
area: macros
where: `dz_macro`, the expander's name qualification
---

**What the rule should be.** §12.4's hygiene: a name the **macro wrote** resolves where the
*macro* is declared, and a name the **caller passed in** resolves where the caller is. That
is what makes a macro a sealed thing — it can call its own module's helpers without the
caller knowing they exist, and the caller's local named `tmp` cannot collide with the
macro's.

**What actually happens.** The expander implements that by **qualifying the macro's own
names with the caller's alias for the macro's module** — a macro in `engine` that writes
`Tally` becomes `engine::Tally` at the call site. So the name is resolved *from the caller*,
through the caller's path, and therefore reaches only what the macro's module **exports**:

- a **private helper** of the macro's module is not reachable, so a macro cannot use one;
- a name the macro's module reaches through **its own dependencies** is not reachable
  either — `rl::…` written inside an `engine` macro resolves at the caller, where `rl` may be
  called something else or not be a dependency at all.

The symptom is an error at the invocation naming something the user never wrote, which is
exactly what principle 4 and invariant 5 say must not happen.

**Why it matters.** It forces a macro's module to make public whatever its macros touch,
which inverts encapsulation: the helper exists *because* it is an implementation detail, and
the macro is the thing meant to hide it. It also means a macro cannot be written against a
dependency, which rules out the most useful kind — a macro wrapping a C library's calling
convention. The workaround, making everything `pub`, works and is wrong.

**What it would take.** Resolve the macro's names **in the macro's module** rather than
rewriting them into the caller's namespace. §12 already gives every token a span that says
where it came from — "a token the macro wrote has the invocation's `#name` as its span, a
captured one its own" — so the information needed to tell the two apart is present and is
already used for diagnostics. What is missing is for resolution to *use* it: a name whose
provenance is the macro resolves against the macro's module's symbol table and dependency
aliases, with the macro's own visibility, and only a captured name resolves at the call site.

That is a change to how `dz_resolve` is entered rather than a change to the expander: the
expanded tree needs to carry, per name, which module's scope to resolve it in. Related to
[T-0033](T-0033-hygiene-renames-by-name-within-one.md), which is the same approximation in
the other direction — hygiene renaming by name rather than by scope — and the two are
probably one piece of work: give expanded names a real scope, and both fall out.

Tests belong in `tests/expand` for the expansion and `tests/workspace/typed/` for the
resolution: a macro in a library module using a private helper, and one using a name from its
own dependency, both called from a module that has neither.
