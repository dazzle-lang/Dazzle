# Tasks

One file per task, `T-NNNN-slug.md`. Read the list with the script rather than the files —
it reads only each file's header, so the whole list costs one line per task:

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

**M12, the stdlib**, chosen by the user on 2026-09-13. Its criterion — a small raylib game
with no C++ of its own — is met: Breakout, 2026-09-15 (`examples/breakout`, its rules played
headless by `tests/run/breakout_sim`). The order the user set on 2026-09-15 is done through
r57 (stderr, `Time`, generic methods, `Random`, `Path` and `File`); what follows is chosen
from `--milestone M12`. Later, by the roadmap: fetched packages (M16), hot reload (M13), and
the release workflow and installer (T-0076).

**When asked what there is to work on**, answer in two parts and say both: the milestone
work (`--milestone M12`, then the other milestones' open tasks), and the tooling that would
make Dazzle easy for an LLM to work in (`--area llm-tooling`).

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

## Out of scope for good

Not tasks, and not deferred: lifetimes — a `&T` cannot be stored in a field and that is the
whole mechanism, a dangling `^T` is the programmer's problem (D-0003, spec §1.1) — and
function overloading until a later language version (D-0005).
