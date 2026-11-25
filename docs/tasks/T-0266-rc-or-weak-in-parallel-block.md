---
id: T-0266
title: An Rc or a Weak in a parallel block: atomic counts, checked by type
milestone: M19
status: open
kind: feature
area: concurrency
---

**What is missing.** §22.4 lets `Rc<T>` and `Weak<T>` cross into a parallel loop and a
`parallel` block, checked as one place per `T`, with counts that are atomic in any program
that runs code at the same time. The block refuses them for now (DZ0599 with §22.4's help),
the user's choice on 2026-11-24 (D-0118), since the counts are plain integers.

**What it needs.** Atomic counts in `std`'s `Rc` and `Weak` for a program with a `parallel`
block, a parallel loop or `spawn!` (a whole-program fact, §8.4), and plain ones otherwise; the
conflict check (`record_parallel`) reading an argument that reaches an `Rc<T>` as reaching the
one place every `Rc<T>` stands for, as the tie analysis already does for guards; then
`holds_rc` no longer refuses it in `parallel_crossings`.

## Where the code is

| Piece | Location |
|---|---|
| The refusal | `crates/dz_check/src/check/parallel.rs`: `crossing`, through `holds_rc` in `crates/dz_check/src/threads.rs` |
| What a call reaches | `crates/dz_flow/src/ties/body.rs`: `record_parallel`, `parallel_conflicts` |
| Every `Rc<T>` as one place | `crates/dz_flow/src/ties/body.rs`: `shared`; `crates/dz_flow/src/ties/report.rs`: the note |
| The counts | `crates/dz_project/src/std/memory/rc_block.dz`, `rc.dz`, `weak.dz` |
