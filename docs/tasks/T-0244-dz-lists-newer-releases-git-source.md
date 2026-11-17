---
id: T-0244
title: dz lists newer releases of a git source pinned by version
milestone: M16
status: open
kind: feature
area: packaging
where: `crates/dz_project/src/fetch.rs` (`resolve_version` lists a repository's tags with `git ls-remote`); `crates/dz/src/main.rs` (the commands)
---

Asked by the user on 2026-11-17, while T-0077 was built ("the compiler would be able to
detect new releases?").

**What is missing.** A git source pinned by `version = "1.93.0"` names a release, and the
repository's other tags say which releases came after it. Nothing reports them: a project
learns of ImGui 1.94 only by looking. A `rev` names one commit and has no "newer"; that is
the trade-off of pinning by commit (D-0110).

**What it would take.** A command, `dz outdated` as Cargo's is named, that lists for each
source pinned by `version` the newer tags of its repository (`git ls-remote --tags`, read
as versions, `v` or not, ignoring pre-releases unless asked), and perhaps `dz update <key>`
to move one to the newest and rewrite its lockfile entry. Reporting it during a build would
make every build touch the network, which a build pinned by its lockfile does not need, so
it belongs in a command of its own.
