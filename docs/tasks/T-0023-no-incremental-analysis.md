---
id: T-0023
title: No incremental analysis
milestone: M9
status: open
kind: deferral
area: editor
where: `dz_lsp/src/lib.rs`, `analyse`
---

**What happens now.** Every time the language server settles after a keystroke, `analyse`
re-runs the **whole project**: parse, macro expansion, resolve, check, flow. Nothing is
reused between runs, even though one file changed and usually one line of it.

**Why it is this way, and why it has been fine.** Invariant 3 — every phase is a pure
function of its input — was written for exactly this, so the caching is possible and
nothing has to be restructured to allow it. It simply has not been built, because at
today's sizes the whole analysis is tens of milliseconds and the editor feels immediate.
A debug-build analysis of `tests/run/hello` is around 61 ms, of which roughly 40 ms is
type-checking `std`'s ~2 200 lines ([T-0025](T-0025-std-type-checked-in-every-analysis.md)).

**Why a game needs it** (from the readiness review, 2026-09-14). The cost is linear in
project size, so it is not a problem until it is one all at once: a real game is tens of
thousands of lines, and at that point every keystroke pays for every file. The symptom is
not a crash but the editor going from immediate to sluggish, which is the thing that makes
people stop using a language server.

**What it would take.** §17.2's **per-module analysis cache**: keep each module's parsed
tree, symbols, type table and HIR keyed on its source text plus the hashes of what it
depends on, and re-run only the modules whose key changed. The blocker is not the cache but
what it would cache: symbols, the type table and HIR are currently built **whole-program**,
so there is no per-module unit to store. Splitting those is the real work, and it is the
same splitting [T-0025](T-0025-std-type-checked-in-every-analysis.md) needs — `std` is just
the module that would benefit first and most, since it never changes.

So the two should be done together, and `std` is the right first case: a module with no
dependencies on user code, whose cache can never be invalidated within a session.
