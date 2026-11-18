---
id: T-0252
title: An agent eval: fixed requests, built offline, measured over time
milestone: M20
status: open
kind: feature
area: llm-tooling
where: none yet; `tests/bench_programs/HISTORY.md` is the model for numbers kept over time
---

M20's criterion, made measurable as Breakout was M12's. Without it, whether a change to the
primer, `dz doc` or a diagnostic helped an agent is a guess.

**What it would take.** A handful of fixed requests, each one line as a user would write it
("a command-line word counter", "a small raylib game with a paddle", …), run by an agent in a
clean sandbox with only the repository's URL, the network cut after setup. For each run:
whether it finished, the turns taken, the compile errors met and which codes, and the
`dz` commands it used. Kept per run, as the benchmarks' history is, so a release can say it
got better.

The codes met most often are the input to T-0059 (diagnostics for habits from Rust and C++).
