---
id: T-0162
title: "#[entry_point]: a function no Dazzle code calls, which names no static's kills"
milestone: none
status: open
kind: feature
area: ownership
---

The user's idea, from settling [T-0136](T-0136-writing-static-unsafe-so-mutable-global.md) on
2026-11-03. A design change, so the spelling and rules are the user's to settle.

**Where the code is**

| Piece | Where |
|---|---|
| Which functions are entry points today: `main`, `#[test]`, `#[bench]` | `entry_point` in `dz_flow/src/ties/body.rs` |
| The rule | §6.7, the bullet "A path may start at a static"; §7.7 |
| Where `#[test]` and `#[bench]` are checked and listed | `check_test_attribute` in `dz_check/src/lib.rs`; `TypeTable::tests`, `benches` |

**What it is.** A kill of a static ripples up every caller's `kills` clause and stops at an
entry point, which nothing in Dazzle calls, so nothing outside it holds a reference into
the static. Today the entry points are `main`, `#[test]` and `#[bench]`. A program has
others: a callback C calls, a function exported to C++ (§15.5), a hot-reloaded module's
entry (M13). Such a function still has to name its static kills, or write them in
`unsafe`.

**What it would take.** An attribute, `#[entry_point]`, on a function Dazzle code may never
call: a call to one is an error, and it may not be stored where Dazzle code could call it
through a value, except as the C callback or export it exists to be. Its static kills then
need no clause, as `main`'s do. Whether `#[export]` implies it is part of the design.
