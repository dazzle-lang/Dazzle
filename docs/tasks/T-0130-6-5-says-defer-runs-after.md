---
id: T-0130
title: §6.5 says `defer` runs after the scope's drops, the order that would read dropped locals
milestone: none
status: open
kind: bug
area: language
---

Found by the review behind [D-0094](../decisions/D-0094-safe-code-has-no-undefined-behaviour.md).
The compiler is right; the spec's sentence is wrong.

**Where the code is**

| Piece | Where |
|---|---|
| The sentence | §6.5: "`defer { ... }` runs a block at scope exit, after that scope's drops, in reverse order of registration" |
| Codegen: a C++ scope guard declared where the `defer` is written | `dz_cpp/src/lib.rs`, `hir::StmtKind::Defer` (~3625), `::dz::make_defer` |
| The ties walk, which replays defers after the drops | `block_scope` in `dz_flow/src/ties/body.rs` (~1673) |

**What it is.** Read literally, §6.5 says a `defer` runs after every drop of its scope, so a
`defer` reading a local declared before it would read a dropped value. The compiler does
the safe thing instead. A `defer` runs after the drops of what was declared after it, and
before the drops of what was declared before it:

```dz
let l = Loud{ name: String::from("first") }
defer { println!("defer sees {}", l.name) }
let m = Loud{ name: String::from("second") }
// prints: drop second, defer sees first, drop first
```

**Why it matters.** The spec is the contract. Anyone who made the compiler match the
sentence would make the program above read a dropped `l`.

**Why it is this way.** The sentence predates codegen's scope guard. The ties walk follows
the sentence: it replays a scope's defers after killing its locals. It reports nothing only
because the locals' state is removed first, so a view read in a `defer` goes unchecked.

**What it would take.** One sentence in §6.5: a `defer` runs where its scope ends, in
reverse order of declaration together with the drops, so every local declared before it is
alive. Then the walk follows the same order, so a view read in a `defer` is checked. A §21
row, since the sentence changes; no program behaves differently.
