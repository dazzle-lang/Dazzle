---
id: T-0030
title: A `type` macro
milestone: M10
status: open
kind: deferral
area: macros
where: `dz_macro`, macro kind validation
---

**What is missing.** A macro declares what it expands to — its *kind* — and since r65 that
kind decides its spelling (§12.5, D-0009): `expr`, `stmt` and `decl` exist, and
**`-> type` is reserved and refused** with `DZ0606`.

```dz
macro array_of -> type { … }        // DZ0606
```

A `type` macro would expand to a type in a type position: a parameter's type, a field's, a
return type, a generic argument.

**Why it is reserved rather than simply absent.** Keeping the kind in the grammar and
refusing it means the name is taken and the error says "not yet" rather than "no such kind",
so adding it later is not a breaking change — and the spelling rules settled at r65 already
account for it.

**Why it matters, and why it is low priority.** The things a `type` macro would do are mostly
done better by generics. Dazzle's generics are definition-site (§8), so a type built from
parameters is a generic struct, not a macro — `List<T>`, not `list_of!(T)`. The remaining
cases are the ones generics cannot spell: a type whose *shape* depends on a value or a flag
(a fixed array whose length is computed, a struct chosen by `#if`), and shorthand for a long
instantiation. Both are rare, and both have workarounds — a type alias, or a generic with a
const parameter where one exists.

So this is genuinely deferred rather than owed, and the useful thing is to know what it would
cost before promising it.

**What it would take, and the hard part.** A macro expands **before types exist** (§12.11),
so a `type` macro's output is tokens that then have to be *parsed as a type and resolved* —
fine in itself. The difficulty is everything downstream of a type's identity:

- **Diagnostics.** A type error mentioning a macro-produced type has to name something the
  user wrote. Spec principle 4 and invariant 5 say errors point at user code; a type whose
  spelling came from an expansion has no such name, so either the macro's invocation stands
  in for it or the error shows the expansion — and the latter is what definition-site
  generics exist to avoid.
- **Hygiene.** §12.4 resolves a macro's own names where the macro is declared, which
  [T-0032](T-0032-name-expansion-resolves-where-macro-declared.md) shows is already
  approximate. A type name is exactly the case where getting it wrong is silent.
- **The editor.** Hover and go-to-definition on a macro-produced type have nothing to point
  at, which is [T-0028](T-0028-completion-from-macro-grammar.md)'s problem again.

None is insurmountable, but together they mean a `type` macro is not a fourth arm in a match
— it is a decision about whether a type may have no written spelling. Worth a decision file
before any code, and worth leaving refused until something real needs it.
