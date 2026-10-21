# Decisions

Every language design question the user has settled, and every one still open, one file
each: `D-NNNN-slug.md`. This is spec §18. **`D-00NN` is the decision the spec, the code and
the build log cite as `18.NN`** — D-0001 to D-0080 kept their numbers when §18's table
became files, so `18.32`, `§18.32` and `D-0032` all name struct inheritance. A new decision
takes the next free id and is cited as `D-NNNN`.

```bash
python tools/track.py decisions                  # all of them, one line each
python tools/track.py decisions --status open    # the questions still open
python tools/track.py show D-0032
python tools/track.py new decision "The question"
```

```markdown
---
id: D-0032
title: Struct inheritance
status: decided
---

The resolution, why, what was rejected, and the spec sections it shapes (§7.3).
```

`status` is `open` or `decided`. **The user decides; Claude proposes.** A decision is
written `decided` only once the user has made the call, in the same edit that adds its spec
§21 revision row. **Do not silently re-decide one**: if implementation shows a decision was
wrong, say so and ask. A reversal is recorded in the decision itself, saying which revision
reversed it and from what (as D-0009 records r63's), alongside its §21 row.
