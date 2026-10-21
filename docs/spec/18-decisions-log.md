# 18. Decisions log

Every design question settled with the user, and every one still open, is a file in
`docs/decisions/`, named `D-NNNN-slug.md`. **`D-00NN` is the decision this spec, the code
and the build log cite as `18.NN`** — the numbers were kept when the table became files,
so `18.32`, `§18.32` and `D-0032` all name struct inheritance. A new decision takes the
next free id and is cited as `D-NNNN`.

`python tools/track.py decisions` lists them, one line each; `docs/decisions/README.md`
says how to write one.

## 18.1 Resolved

The decisions with `status: decided` — `python tools/track.py decisions --status decided`.

## 18.2 Still open

The decisions with `status: open` — `python tools/track.py decisions --status open`.
