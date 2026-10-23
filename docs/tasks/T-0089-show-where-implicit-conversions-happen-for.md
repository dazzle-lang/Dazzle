---
id: T-0089
title: Show where implicit conversions happen, for LLMs and debugging
milestone: M12
status: open
kind: feature
area: llm-tooling
where: `dz_check` (the `ImplicitAs` call `Body::implicit_as` inserts), `dz` CLI, `dz_lsp`
---

**What is missing.** Since r78 an argument may convert on its own through `ImplicitAs<T>`
(§7.2, D-0089): `c_len(name)` is really `c_len(ImplicitAs::convert(&name))`. The source does
not show it, and no tool does either. The checker records it, as a `Callee::Method` named
`convert` on the trait, but nothing reads it back out except the C++, where it is
`ImplicitAs_cstr__std_String__convert(name)`.

**Why it matters.** An implicit step is exactly what a reader, human or LLM, cannot see,
and what makes a bug hard to find: a conversion that copies, or picks the wrong one of a
type's two targets, or a `convert` with a side effect. An LLM working from the source alone
will reason as if the argument were passed as written. It needs a way to ask "what does
this call actually do with its arguments?" without reading generated C++.

**What it would take.** Three readers of the same fact, cheapest first:

- **`dz check --format=json`**: a list of the conversions in the analysed code, each with
  its span, source type, target type and the `impl` that does it (file and line). An LLM
  can ask for it directly; this is the one to build first.
- **An editor inlay hint** at each converted argument (`name` shows `.convert() → cstr`),
  and hover on the argument naming the `impl`. Same data, through `dz_lsp`'s `ide.rs`.
- **`dz expand`** could print the program with each conversion written out, as it already
  does for macros, so a whole file reads as the compiler sees it.

All three read the checked HIR: a `Call` whose callee is `ImplicitAs`'s `convert` and whose
span is the argument's own. Marking it as implicit (a flag on the node, or a side table of
expression ids) lets them tell it apart from a `convert` someone wrote by hand.
