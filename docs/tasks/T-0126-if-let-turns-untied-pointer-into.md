---
id: T-0126
title: `if let` turns an untied pointer into a reference without `unsafe`
milestone: none
status: done
kind: bug
area: ownership
---

Listed in [D-0094](../decisions/D-0094-safe-code-has-no-undefined-behaviour.md) as needing
no decision: D-0093 point 11 already decided the rule, and
[T-0114](T-0114-untied-pointer-becomes-reference-only-in.md) built it for `or` only.

**Where the code is**

| Piece | Where |
|---|---|
| The check, for `or` | `dz_flow/src/ties/body.rs`, `hir::ExprKind::Or` (~2460): `or_pointer` (in `ties.rs`), `unsafe_depth`, `vouched`, `tied_pointer` |
| The check, for `if let` | `dz_flow/src/ties/body.rs`, `hir::ExprKind::IfLet` (~2406) |
| The diagnostic, `DZ0418` | `untied_conversions`, reported in `dz_flow/src/ties/report.rs` (~159) |
| Tests | `tests/flow/untied_pointer.dz`; `unsafe` written in `tests/run/dynamic`'s `grow_through` and `typed/`'s `if_let_forms` |

**Done (2026-11-03).** The `IfLet` arm asks what `Or` asks, for every binding form; the
offered fix wraps the operand alone, so the branches stay checked. `docs/log/r81-ties.md`.

**What it is.** Spec §5.2 says an untied pointer, such as one read from a field, becomes a
reference only in `unsafe`. `ctx.btn or return` is `DZ0418`. `if let` is the same
conversion, and nothing checks it:

```dz
struct Slot {
    pub target: ^mut Node = null
}

fn poke(slot: &Slot) {
    if let &mut n = slot.target {       // accepted, with no `unsafe`
        n.name.push_str("!")
    }
}
// with `slot.target` set to a local that has since dropped, this wrote through a dangling
// pointer; only `--track-memory` noticed, as a double free
```

**Why it matters.** It undoes T-0114: "everything unchecked is written `unsafe`" is not true
while `if let` is a way around it.

**What it would take.** The `IfLet` arm does what `Or` does, for every binding form:
`&v`, `&mut v`, and `v`, which reads through the pointer. Add the case to
`tests/flow/untied_pointer.dz`, and write `unsafe` in `tests/run/dynamic`'s `grow_through`,
whose `if let &mut s = slot.shape` reads a `^mut Shape` field. Compiler work only
(`docs/log/`).
