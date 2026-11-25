---
id: T-0267
title: A parallel block hands back its calls' results
milestone: M19
status: open
kind: feature
area: concurrency
---

**What is missing.** Each call in `parallel { … }` returns nothing (DZ0597), the user's choice
for its first step (D-0118); a result is written through a `&mut` argument. Rust's
`rayon::join` returns a tuple, and so could the block: `let (a, b) = parallel { f() g() }`.

**What it needs.** A language decision first: the spelling, and whether a block with results
is an expression. Then a result that owns memory is built on the heap, or where the caller
wants it, never in a worker's region (§22.6), and a fallible call's failure has somewhere to
go.

## Where the code is

| Piece | Location |
|---|---|
| The refusal | `crates/dz_check/src/check/parallel.rs`: `parallel_stmt` |
| The emitted block | `crates/dz_cpp/src/lib.rs`: `Emitter::parallel` |
