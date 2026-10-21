---
id: T-0022
title: More code actions, and inlay hints for implicit `drop`s
milestone: M9
status: open
kind: deferral
area: editor
where: `dz_lsp/src/ide.rs`
---

**What this row is.** Items 6 and 7 of §17.4, the two editor features not yet built.

**Code actions** — offered on a selection or a cursor position rather than attached to a
diagnostic:

- **fill a `switch`'s arms** from the union or enum being matched;
- **insert `move()`** where a value is being copied and the copy is not wanted;
- **add `impl Drop`** for a type that owns something.

Note the overlap with [T-0073](T-0073-remaining-quick-fixes.md), and the difference: a
**quick fix** hangs off a diagnostic the compiler already produced, and the first two of
those are done. A **code action** is offered where there is no error at all — refactoring
rather than correcting. Filling a `switch` appears on both lists for that reason: as a fix
when the compiler has reported a missing arm, and as an action on a `switch` that is
complete but about to be extended.

**Inlay hints for implicit `drop`s** are the more interesting half. Dazzle's ownership model
is three special members and a `drop` that runs at a scope's end (§6.5), and *where* those
drops happen is the thing a programmer has to hold in their head. An inlay hint would show it
instead:

```dz
fn load() {
    let tex = Texture("hero.png")
    let snd = Sound("hit.wav")
    use_them(&tex, &snd)
}                               ‹ drop snd, tex ›
```

**Why it matters.** This is the feature that teaches the language. C++ programmers know
destructors run at scope end but not in what order; Rust programmers expect drops but not
Dazzle's rules about `move()` resetting its source (D-0033). A hint showing exactly what dies
where, in order, makes the model visible rather than something you learn by being surprised.
That is worth more than any one quick fix, and it is the item on this list to build first.

**What it would take.** `dz_flow` already computes every drop point — it has to, to emit
them — so the information exists and the work is exposing it: a query on an `Analysis`
returning drop sites with their spans and the locals dropped in order, then the LSP's
`textDocument/inlayHint` over it, and a setting in the VS Code extension since inlay hints
are the kind of thing people want off as often as on. `ide.rs`'s queries take an `Analysis`
and an offset and are tested without a client, so the test is a unit test over positions.
