---
id: T-0065
title: Checking a game without looking at it
milestone: none
status: open
kind: feature
area: llm-tooling
---

**Checking a game without looking at it.** A model cannot see a raylib window.
`tests/run/breakout_sim` already plays the Breakout rules headless; make that the
documented pattern, and add `dz run --frames N` (exit cleanly after N frames) and state
snapshot tests built on the M11 reflection JSON writer, so "move the player left" can be
checked against a stored snapshot.
