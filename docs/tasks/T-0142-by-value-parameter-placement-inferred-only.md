---
id: T-0142
title: A by-value parameter's placement is inferred only within its module
milestone: none
status: open
kind: feature
area: ownership
---

From [T-0117](T-0117-memory-placed-by-compiler-regions-result.md): "Where a by-value
parameter ends up is inferred within a module; across modules, and through anything the
compiler cannot see, the default is the heap." r90 settled that placement within a module
is decided per module, so the editor can run it.

**Where the code is**

| Piece | Where |
|---|---|
| Where a value ends up: escapes, holders | `Escape`, `fate`, `holder` in `dz_flow/src/placement.rs` |
| The summary within a module: which parameters stay in the call | `Staying`, `staying_params`, `owner_module` in `dz_flow/src/placement.rs`; checked by `analyse_regions` (`params`, `stays_in_call`) in `dz_flow/src/ties/body.rs` |
| Per-module analysis and its cache | `dz_driver/src/lib.rs` (`Analysis::placements`) |

**What it is.** A value passed by value to a function in another module goes to the heap,
because the caller cannot see whether the callee only borrows it, drops it, or keeps it.

Within a module it is inferred since 2026-11-02 (`Staying`): a parameter the callee only
reads, borrows, copies or drops, or hands to another such parameter, *stays in the call*, and
what is passed to it keeps the caller's region. The summary is computed from bodies over the
whole program, and `Staying::stays` refuses to use it across a module boundary. What is left
is to export it with the module's signatures, so that a call into another module can use it
too.

**Why it matters.** A game split into modules (engine, game, UI) loses region placement at
every module boundary. Splitting a program into modules is how §3 wants programs organised.

**What it would take.** A placement summary per function, exported with the module's
signatures as `kills` clauses are, but inferred rather than written: which by-value
parameters the function only borrows or drops, and which it stores or returns. It is
computed from the module's own code and its dependencies' summaries, so each phase stays a
pure function of its input (invariant 3) and the editor can still run placement per module.
Compiler work (`docs/log/`). Whether the summary is shown, as a clause is, is a small design
question.
