---
id: T-0028
title: Completion from a macro's grammar
milestone: M9
status: open
kind: deferral
area: editor
where: `dz_lsp/src/ide.rs`, `completion`
---

**What works and what does not.** §17.4 item 5. Inside a macro invocation, completion
offers the **captured code** — a state machine's locals, `btn.` and its methods — because
that code is ordinary Dazzle and the checker sees it after expansion. What it does not offer
is the **macro's own grammar**: the pattern's literal words, and which part of the pattern the
cursor is in.

```dz
state_machine! {
    state Idle { … }        // `state` is the macro's word; completion does not suggest it
    │                       // cursor here: the editor cannot say what may come next
}
```

**Why it matters.** A macro with a grammar of its own is a small language, and the editor
treats it as unstructured text. That is the opposite of the trade §12 makes: macros are
*declared* with a `syntax` and a `pattern` precisely so the compiler knows the grammar
rather than taking a token soup — and then the one consumer that would most benefit from
knowing it does not use it. For a user, a macro's vocabulary is the least discoverable thing
in the language: there is no signature to hover, and the only way to learn `state` is to read
the macro's declaration.

This is also one of the `llm-tooling` concerns in a different guise: a model writing a macro
invocation is guessing at the grammar, and guessing is where it invents
([T-0061](T-0061-api-dump-dz-api-module.md) is the same problem for module APIs).

**What it would take.** The grammar is **in the macro's declaration**, parsed and checked
(§12.1–12.3), so nothing has to be inferred. Three pieces:

1. Map the cursor's offset to a **position in the pattern** — which fragment it is inside,
   or which alternative of a repetition. This is the real work: the expander matches a whole
   invocation, and what is wanted is a partial match over a half-typed one that says "we are
   here in the pattern". The lexer and parser never bail (invariant 1), so the tokens are
   available; the matcher needs a mode that reports how far it got rather than failing.
2. From that position, offer the pattern's **literal tokens** and the **fragment kinds**
   expected next (§12.2) — an `expr` position should fall through to ordinary expression
   completion, which already works.
3. Signature help, which falls out of the same position: show the pattern with the current
   fragment highlighted, the way a function's parameters are shown.

`ide.rs`'s queries take an `Analysis` and an offset and are tested without a client, so each
of these is a unit test over positions in a `tests/expand` fixture.
