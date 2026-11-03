# Tasks

One file per task, `T-NNNN-slug.md`. **`docs/tasks.md` is the backlog**: every open task,
one line each, grouped by milestone — read it first. It is generated from these files by
`python tools/track.py index` and never edited by hand; a hook rewrites it whenever a task
file changes, and `check` fails while it is stale. To change the backlog, change a task file
(or the `## Now` section below, which it copies). For other views, the script reads only
each file's header:

```bash
python tools/track.py tasks                      # every open task
python tools/track.py tasks --milestone M12      # one milestone's open tasks
python tools/track.py tasks --area llm-tooling   # one area; --kind deferral|design|…
python tools/track.py tasks --status all         # done and dropped ones too
python tools/track.py milestones                 # open and done counts per milestone
python tools/track.py show T-0050                # one task in full
python tools/track.py new task "Title" --milestone M12 --area stdlib --kind feature
python tools/track.py check                      # headers, ids, milestones, §-references
```

## Now

**Memory placed by the compiler, then safe code made sound**, chosen by the user on
2026-11-01. Automatic placement matters most (T-0117 and what follows it), and threads can
wait (T-0011 stays in v2). A review that day found safe programs that still reach freed
memory. D-0094 proposes the rules that close each hole, for the user to decide. The order
proposed:

1. T-0117 is done (2026-11-02): the gate kept regions. Peak memory, the loop and safety
   pass; with r105 in, `astar` and `words` are faster than the heap, and `expr` is even,
   its parser nodes kept on the heap by the loop rule.
2. The holes the placement check shares, since a placement is checked like a reference:
   T-0123, T-0124, T-0125 and T-0127 are closed (r103), and T-0131 makes a wrong placement
   show in tests (all 2026-11-02).
3. Frame regions (T-0139) turned out not to be needed yet (2026-11-02): the function
   called each frame already has its region. Two gaps that kept frame data on the heap are
   closed instead (`format!`'s text, and a value handed to a parameter that stays in the
   call), and a field replaced every frame joined T-0120. T-0119 is done (r104,
   2026-11-02): ordinary code names no allocator, and a container reads where it was
   placed as `#placement`. The speed hand-placed `Temp` gave `words` and `expr` waits
   for T-0145 and T-0146. T-0118 is done (2026-11-02): a drop that only frees is skipped
   when its region frees everything the value owns; the cases it still leaves are T-0147.
   T-0145 is done too (2026-11-02): a value stored into a local container is built where
   the container lives, so a `List<String>` filled in its region skips its drop. T-0149
   and T-0140 are done (r105, 2026-11-02), asked for by the user before T-0117's gate since
   they change what it measures: a map filled under a lookup that found nothing, `format!`'s
   text in a loop and a program's own container keep their values in the region, and a copy
   is built where it is placed. `words`' keys and `expr`'s names are now in `main`'s region.
   A container lent to a function is left as T-0150, a design question. The user's idea of
   a checked marker for a drop that only frees is T-0148, for later.
4. The rest of D-0094, decided in full on 2026-11-03 (r107) and to be built: T-0128,
   T-0129, T-0126, T-0130 and T-0135 are done (r108–r111, 2026-11-03); then T-0136, and
   §1.1's promise (T-0160); its tests (T-0132 to T-0134), and the placement gaps (T-0141, T-0142, T-0146, T-0147, T-0150).
5. A game of ten thousand lines (T-0138), before deciding how exact a clause must be
   (T-0137).

**M12, the stdlib**, chosen by the user on 2026-09-13. Its criterion — a small raylib game
with no C++ of its own — is met: Breakout, 2026-09-15 (`examples/breakout`, its rules played
headless by `tests/run/breakout_sim`). The order the user set on 2026-09-15 is done through
r57 (stderr, `Time`, generic methods, `Random`, `Path` and `File`); what follows is chosen
from M12's open tasks. Later, by the roadmap: fetched packages (M16), hot reload (M13), and
the release workflow and installer (T-0076).

**When asked what there is to work on**, answer from `docs/tasks.md` in two parts and say
both: the milestone work (M12 first, then the other milestones' open tasks), and the tooling
that would make Dazzle easy for an LLM to work in (the `llm-tooling` rows under `none`).

## A task file

```markdown
---
id: T-0050
title: Associated-type projection
milestone: M12
status: open
kind: deferral
area: checker
where: `dz_check/src/check.rs`, `iterate_item`
---

What is missing or wrong, why it matters, and what has been tried.
```

**The body is written for someone who has not met the problem.** The title is a reminder;
the body is the explanation, and it is what the backlog's link leads to. Four things, in
this order, each under a bold lead-in so a reader can skip to the one they want:

1. **What it is** — the feature or the defect, *spelled out*, with a short Dazzle snippet
   showing the refused or wrong form and the working one beside it. Do not assume the reader
   knows what the title's words mean: "format specifiers" meant nothing to the user who had
   to choose whether to schedule it, and that is the bar to clear.
2. **Why it matters** — concretely, in terms of a program someone would write. "A game needs
   it for X" beats "it is a gap". If it does not matter much, say so: a row that admits it is
   low value is more useful than one that implies everything is urgent.
3. **Why it is this way** — for a deferral, what was tried and what blocked it; for a
   feature, why it has not been built. Name the diagnostic code where there is one.
4. **What it would take** — the files and functions, and the **decision the user has to
   make** if there is one, with the options and what each costs. Say explicitly whether it is
   a language change (a §21 revision row and usually a `D-NNNN`) or only compiler work
   (`docs/log/`).

Link related tasks as `[T-0050](T-0050-associated-type-projection.md)` — a relative link to
the sibling file, so it is clickable from the backlog and in an editor. `check` verifies every
`T-NNNN` and `D-NNNN` mentioned anywhere under `docs/` exists.

**Do not quote the `title:` value.** `header` splits on the first colon and keeps the rest
verbatim, so backticks and a second colon are fine as they are; quotes would become part of
the title and appear in the backlog.

| Field | Values |
|---|---|
| `milestone` | a row of the roadmap, spec §19 (`M9`, `M12`, …), `v2` for after the first release, or `none` for not scheduled. A milestone is a group of tasks; when milestones become versions, that mapping goes in §19 and no task changes |
| `status` | `open`, `done` (add `done: r65, 2026-10-22` — the revision or commit and the date), or `dropped` (add a line saying why) |
| `kind` | `feature`; `deferral`, something the code knowingly postpones; `design`, which changes the language and is the user's decision (it gets a decision file and a spec §21 row); `bug` |
| `area` | free text, kept to the words already in use: `ownership`, `text`, `generics`, `interop`, `checker`, `codegen`, `stdlib`, `macros`, `resolve`, `editor`, `bench`, `packaging`, `concurrency`, `tooling`, `llm-tooling`, `language` |
| `where` | for a deferral, the file and function that postpone it |

**Rules.**

- **Done is a status, not a deletion.** Set `status: done` and record what was built in the
  milestone's file in `docs/log/`. Done tasks are how a milestone, and later a version, says
  what it contains.
- **A deferral has a comment in the code naming its task** (`// deferred: T-0034`), so
  either end finds the other. Comments written before tasks had ids name the milestone.
- **Before claiming a milestone is complete**, `python tools/track.py tasks --milestone M…`
  must print nothing, or every row left must be moved to another milestone with the user's
  agreement.
- Ids are never reused or renumbered; `new` takes the next free one.
- **A task whose code spans several files keeps a "Where the code is" table** near the
  top: each piece, and the file and functions that hold it ([T-0107](T-0107-design-ties-kills-clauses-compiler-placed.md)
  has one). Whoever works on it updates the table in the same change that moves the code, so
  the next session reads where to work instead of searching for it.

## Out of scope for good

Not tasks, and not deferred: lifetime parameters — a tie belongs to a type, never to a
struct that holds references (§6.7, D-0091), and a reference lives only where references
live, a view struct included (D-0003, D-0093); a raw `^T` is the programmer's problem, inside
`unsafe` (spec §1.1) — and function overloading until a later language version (D-0005).
