---
id: T-0136
title: Writing a static is `unsafe`, so mutable global state is never checked
milestone: none
status: done
kind: design
area: ownership
---

**Done** (r112, 2026-11-03). The user settled three points that day. A write that kills
nothing is safe with no clause. A static's kill ripples up callers' clauses and stops at
entry points: `main`, `#[test]`, `#[bench]` ([T-0162](T-0162-entry-point-function-no-dazzle-code.md)
is a later `#[entry_point]`). A function whose clause names a static fits a function type
or trait method only if theirs names it too. A kill inside `unsafe` needs no clause, which
keeps std's arena and C callbacks working. Build log: `docs/log/r81-ties.md`.

Point 7 of [D-0094](../decisions/D-0094-safe-code-has-no-undefined-behaviour.md), decided
on 2026-11-03 (r107) as option A below: a `kills` clause may name a static. It revises
D-0046.

**Where the code is**

| Piece | Where |
|---|---|
| The rule | §6.7, the bullet "A path may start at a static"; §7.7 |
| A root written `world::SCORES` | `TiePath::module` in `dz_ast`; `parse_tie_path` in `dz_parse/src/item.rs` |
| A static root resolved where the function is declared, and kept on its signature and function types | `tie_statics`, `fn_sig_inner` and the `TypeKind::Fn` lowering in `dz_check/src/lib.rs`; `FnSig::statics`; `FnKills::Known::statics` and `covered` in `dz_hir/src/ty.rs` |
| `Root::Static`, its visibility (`DZ0563`) | `resolve`, `static_root`, `static_visible` in `dz_check/src/ties.rs` |
| A static as a place: a pseudo-local per static | `static_local`, `static_of`, `local_ty` and `Bindings::get` in `dz_flow/src/ties.rs`; `hir::ExprKind::Static` in `dz_flow/src/ties/body.rs` |
| Kills counted against the clause; entry points; `unsafe` vouching | `analyse` (the `actual` filter), `entry_point` and `kill` in `dz_flow/src/ties/body.rs` |
| A fix that names a static as the file would | `tie_path_held` in `dz_flow/src/ties/report.rs`; `Resolution::spelling_in_file` |
| Tests | `tests/flow/static_kills.dz`, static cases in `use_after_kill.dz` and `clean.dz`; `typed/` and `typeerrors/` (`ACCOUNTS`); `tests/run/statics`; `a_clause_names_a_static_in_another_module` in `dz/tests/run.rs`; `tests/parse/ties.dz`, `tests/fmt/ties.dz` |

**What it is.** Reading a static is safe; changing one is `unsafe` (D-0046). A safe
function can change a static inside `unsafe`, and its caller can hold a reference into
that static across the call. This is accepted, and reads freed memory:

```dz
struct Scores {
    pub list: List<i32> = List<i32>()
}

static SCORES: Scores = Scores{}

fn add(v: i32) {                    // safe to call
    unsafe { SCORES.list.push(v) }
}

add(41)
let top = &SCORES.list[0]           // reading a static is safe
for i in 0..100 { add(i) }          // moves the array `top` points into
println!("{}", top)                 // accepted: prints 41, from freed memory
```

The `unsafe` in `add` is where the fault is, by §1.1's rule. But no `unsafe` block can be
correct here, since it cannot know who holds a reference. A mutable static that owns memory
cannot be used soundly at all.

**Why it matters.** It matters little today: a static holds constants only (§7.7), and
std's uses (the temporary arena, the tracker) sit behind safe interfaces that hand out no
references into them. It will matter if global game state lives in statics.

**What it would take.** A choice:

- **A. A `kills` clause may name a static** (proposed): `fn add(v: i32) kills
  SCORES.list@items`. A write that its clause covers becomes safe. Tie paths gain a root
  for a static, and every caller's references into it are checked like any other.
- **B. A reference into a static that owns memory needs `unsafe`.** The hole is closed and
  the statics stay unchecked.
- **C. Keep D-0046**, and document the hole.

A language change: §7.7 and §6.7, and a §21 row.
