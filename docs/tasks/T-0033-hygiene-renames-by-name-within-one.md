---
id: T-0033
title: Hygiene renames by name within one expansion
milestone: M10
status: open
kind: deferral
area: macros
where: `dz_macro`, the hygiene renamer
---

**What hygiene is for.** A macro that writes `let tmp = …` must not collide with a caller's
own `tmp` (§12.4). The expander achieves that by **renaming** the identifiers the macro
wrote to fresh ones, so the macro's `tmp` and the caller's are different names.

**What goes wrong.** The renaming is **by name within one expansion**, not by scope. So a
macro that writes one identifier in two roles — once as a local and once as a module-level
name — has **both** treated as the local and renamed together:

```dz
macro tally -> stmt {
    expand {
        let count = 0              // a local the macro writes
        count += Tally::count()    // `count` here names something at module level
    }
}
```

The second `count` is renamed along with the first, so it no longer names the module-level
thing, and the error — if there is one — points at a name the user never wrote.

**Why it matters.** Rarely, because the shape is unusual: it needs a macro that writes the
*same spelling* at two different scopes within one expansion. But when it happens it is the
worst kind of failure — silent, or reported against an invented name — and it is not
something a user could diagnose, since the thing that went wrong is invisible in their
source. It also constrains macro authors in a way nothing tells them about: the rule in
practice is "do not reuse a spelling across scopes inside an expansion", which is nowhere
written down.

**What it would take.** Rename **by binding** rather than by spelling: the expanded tree
needs to know, per identifier, which binding it refers to — or at least which scope it was
written in — rather than matching on the text. That is the same missing information
[T-0032](T-0032-name-expansion-resolves-where-macro-declared.md) needs (a name's provenance
and the scope it resolves in), so **the two are one piece of work**: give every name in an
expansion a real scope, then hygiene renames the ones bound inside the expansion and leaves
the rest, and T-0032's module-level resolution falls out of the same data.

The pieces are partly there: §12 gives every token a span distinguishing what the macro
wrote from what was captured, which is the provenance half. What is missing is the scope
half.

A `tests/expand` case with exactly the shape above pins it, and the dump is the right place
to assert it since the defect is visible in the expansion before any type checking.
