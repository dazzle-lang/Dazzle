---
id: T-0245
title: Try to break D-0096 before deciding it
milestone: M19
status: open
kind: design
area: concurrency
where: `docs/decisions/D-0096-threads-safe-multithreading-built-on-ties.md` (the design); `tests/bench_programs/ecs` (the ECS it is checked against)
---

The first step of M19, set by the user on 2026-11-18 when threads were put in 0.3. Nothing
is built until it is done, since it decides how much of the rest is checking and how much is
documentation.

[D-0096](../decisions/D-0096-threads-safe-multithreading-built-on-ties.md) is agreed in
direction (2026-11-10) but not decided. Its own section "Before deciding: try to break it"
is this task:

1. **Port four or five real systems on paper**: sprite animation, the player, bullets,
   something with random access to another entity. `tests/bench_programs/ecs` is now a
   real Dazzle ECS to port them against, where D-0096 was written against the user's Odin
   one.
2. **Write the programs that must be refused**, D-0096 lists seven, and find for each the
   rule that refuses it. Each becomes a `tests/errors` case once the rule is built.
3. **Bring the open questions to the user**, one by one: the names of sendable and
   shareable, the clause naming plain statics (`uses`?), `A::Target` or `target_of<A>`,
   `ref` as the keyword, the marker on `join`, what a panic on a worker does, and whether
   closures are needed after all.

**Done when** every refused program has a rule, the open questions are answered, and D-0096
is `decided`, with the spec sections it lists (§1.1, §6.7, §7.7, §8, §14.5, §14.6, §15)
rewritten and a §21 row. The building is then [T-0011](T-0011-threads-mutex-atomics-job-system.md).
