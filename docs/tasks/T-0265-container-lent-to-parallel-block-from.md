---
id: T-0265
title: A container lent to a parallel block from further out, placed so no arena needs a lock
milestone: M19
status: done
kind: feature
area: concurrency
---

**Done 2026-11-25, r152: `lends p` (spec §22.12, D-0119).**

**The problem.** A container can reach a `parallel` block (spec §22.3, D-0118) through a
parameter: `frame(w: &mut World)` with `parallel { fill(&mut w.a) fill(&mut w.b) }`, where
`frame`'s caller put `w`'s lists in its region. Two calls then grow one region at once, and a
region is a bump allocator with no synchronisation. Placement sends a block's *own* locals to
the heap, but `frame` cannot move `w`, and its caller cannot see the block.

**What was tried first (r151).** While any block ran, a change to a region or an owner arena
through an `Allocator` handle took a lock: one for every allocator, then, the user's choice,
one per arena. Correct, but the check alone, a relaxed load and a branch per allocation, cost
up to 8% in programs that ran no block (`words` on g++, `astar` 5%, `frame` 3% on clang++),
measured against the compiler before it; emitted only for a program with a block, every
program without one was within noise, `frame` within 1.5% from the lock word each arena then
carried.

**What replaced it.** The user chose to make it static and written: `lends w` on `frame`, so
its callers put `w` on the heap. The tie walk records what a body lets another thread change
(`BodyAnalysis::lent`): a write by a call in a block, or an argument to a callee's `lends`
parameter, followed through references, only where what is reached may own memory. A
parameter among them must be in the clause (DZ0425, DZ0426, with `dz fix`); any other local,
placement puts on the heap. The check, the locks and the lock words are gone.

**Left open.** `lends` on a function type; until then a function that lends is called by name
(DZ0428).

## Where the code is

| Piece | Location |
|---|---|
| The keyword and the clause | `crates/dz_lex/src/token.rs` (`Lends`); `crates/dz_parse/src/item.rs` (`parse_fn`); `FnDecl::lends` in `dz_ast`, `FnSig::lends` in `dz_check`; `dump::lends_str` |
| What a clause may name (DZ0427) | `crates/dz_check/src/ties.rs`: `lendable`, from `check` |
| Never a function value (DZ0428); within a trait's or a base's (DZ0429) | `crates/dz_check/src/check.rs`: the path's `SymbolKind::Fn` arm; `lends_within_ceiling` |
| What a body lends | `crates/dz_flow/src/ties/body.rs`: `record_parallel`, `record_lends`, `lend_to_thread`, `may_grow`; `BodyAnalysis::lent` |
| Held to the body (DZ0425, DZ0426) and its fix | `crates/dz_flow/src/ties/report.rs`: `report_lends` |
| What is lent goes to the heap | `crates/dz_flow/src/placement.rs`: `lends_anything`, `Cx::lent`, `escape_of_all` |
| Tests | `tests/errors/lends_*.dz`, `lending_function_as_a_value.dz`; `tests/workspace/typed` and `typeerrors`; `tests/flow/parallel_conflict.dz`; `dz_flow`'s `what_is_lent_to_a_block_is_built_on_the_heap`; `tests/run/parallel` |
