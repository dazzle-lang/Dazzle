---
id: T-0219
title: "Loop hints: what a loop does on every pass that it could avoid"
milestone: none
status: open
kind: feature
area: tooling
---

**For 0.3**, the user's call on 2026-11-14. The user's idea: Dazzle detecting hot loops and
helping find why one is slow.

**What it is.** Saying a loop *is* slow needs the input, the caches and the machine, so the
compiler cannot. What it can say, from analyses it already runs, is what a loop does on
every pass that it could avoid, each with the reason and the fix:

| On every pass | Known from |
|---|---|
| a `List` indexed through the list, not a slice taken before the loop, and the call that kills it | `dz_flow::views` (T-0215) |
| `for e in xs` copying an element that owns memory, where `&e` would not | the checker: the binding's type and the element's `copy` |
| an allocation on the heap, and why it is not in a region | placement (`dz placements`, spec §14.6) |
| a drop that runs although its region frees the memory | `dz_flow::release` (T-0118, T-0218) |
| a call through a function value, `virtual`, or a trait object | the callee in the HIR |

Shown as `dz hints` on the command line, `--format=json` for tools and LLMs, and in the
editor as hints on the loop, as placement's are.

**And the time itself.** The generated C++ carries `#line` directives, so a sampling profiler
(perf, VTune, Instruments) already charges time to `.dz` lines. A `dz profile` could build
with debug information, run one, and list the hottest loops beside their hints.

**Where the code is.** Nothing yet. The analyses are in `crates/dz_flow` (`views.rs`,
`placement.rs`, `release.rs`); placement's editor hints in `crates/dz_lsp` are the model for
showing them.
