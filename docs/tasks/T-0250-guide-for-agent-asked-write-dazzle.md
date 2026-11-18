---
id: T-0250
title: A guide for an agent asked to write a Dazzle program, and the README's line to it
milestone: M20
status: open
kind: feature
area: llm-tooling
where: `README.md` (the line at the top); a new `docs/agents/write-a-program.md`; `CLAUDE.md` (one line the other way)
---

Asked by the user on 2026-11-18, with the request they want to work: "use this language
https://github.com/dazzle-lang/Dazzle to create a simple game". An agent given a GitHub URL
reads the README first, so the README must send it somewhere.

**Two audiences, two places.** An agent working *on* the compiler reads the root
`CLAUDE.md` (other agents read a root `AGENTS.md`, by convention). An agent asked to *use*
Dazzle must not be given the compiler's working notes, and the compiler's agents must not be
told to install `dz` and make a project. So:

- The guide is **not** a root `AGENTS.md` or `CLAUDE.md`, which agents load automatically
  as instructions for the repository. It lives where nothing loads it on its own:
  `docs/agents/write-a-program.md`.
- The README's very first lines point to it **by task**, not by "if you are an agent": if
  you were asked to *write a program in Dazzle*, read the guide; if you are working on the
  compiler, `CLAUDE.md` applies and the guide does not.
- `CLAUDE.md` says the same the other way: the guide is for Dazzle's users.

**What the guide says**, short, since an agent may read only the start:

1. Check `dz --version`. If it is missing, install the release build (`install/`), which
   needs no Rust; clone and build only if that fails, into a tools folder such as
   `~/.dazzle`, never into the project.
2. Create the project **outside** the Dazzle checkout, with `dz new --agent <name>`
   (T-0064): `CLAUDE.md` files load from the working directory and its parents, so a project
   inside the checkout would load the compiler's notes.
3. Fetch what the project's dependencies need once (`dz` builds a git source from its
   lockfile, T-0077); from then on no network is needed.
4. Work from `dz doc`, `dz api`, `dz query`, `dz check --format=json` and `dz explain`.

Also an `llms.txt` at the website's root pointing to the same guide (T-0251).
