---
id: T-0251
title: The website is generated from this repository's docs, not written by hand
milestone: M20
status: open
kind: feature
area: tooling
where: the website lives in a repository of its own, outside this one; `docs/spec/`, `docs/decisions/`, `docs/releases/`, `docs/spec/19-roadmap.md` here
---

Said by the user on 2026-11-18: the website is separate from this repository, so every
decision, new feature and roadmap change has to be written into it by hand. It falls behind,
and once `dz doc` (T-0249) ships the same documents inside `dz`, the two would disagree.

**What it would take.** One source: this repository's `docs/` is what the website shows, built
from it, by the website's build reading a checkout of this repository at a tag, or by a
step here that exports the pages the website imports (`dz doc --export <dir>` would serve
both). The pages written only for the website (the landing page, the pitch) stay hand-written
there; the spec, the decisions, the roadmap, the release notes and the examples do not.

Until it is done, a change to any of those is copied to the website by hand.
