---
id: D-0110
title: "Git sources: version or rev, pinned in dazzle.lock, fetched by dz and never by the editor"
status: decided
---

**A dependency or a `[cpp.libs]` recipe may come from a git repository: `git = "https://…"`
with exactly one of `version`, a release tag (`v1.93.0` or `1.93.0`), or `rev`, a commit
written in full. `dazzle.lock`, beside the root manifest and checked in, records the commit
each resolved to. `dz build`, `dz run` and `dz check` fetch what the per-user cache lacks by
running `git`; the editor never fetches. A fetched recipe may have a CMake build, which runs
as a vendored one's does. Chosen by the user on 2026-11-17, in T-0077 ("git + rev. or git +
version"; CMake "allow, lockfile-pinned"; fetching "on build/check, rust does that too");
spec §3.2, §15.1, r142.**

**Before.** Every dependency was a `path`, and a C library not in the repository was a git
submodule under `examples/vendor/`, so `git clone` without `--recursive` could not build the
examples, and each library's version was recorded only in the submodule's commit.

**What was weighed.**

- **What pins a source.** A `version` names a release, which a later tool can compare with
  the repository's newer tags (T-0244); a `rev` names one commit, for code with no release.
  A branch is refused: it names different code from one day to the next. Whichever is
  written, the lockfile holds the commit, so a tag moved upstream changes nothing until
  its entry is deleted. The commit is git's hash of the whole tree, so the lockfile records
  no separate content hash.
- **An archive URL with a hash**, for libraries that publish release tarballs, was offered
  and not taken: git covers every library the examples use, and one way is less to learn.
- **What runs.** Fetching runs `git`, and nothing from the fetched repository runs except a
  recipe's CMake build, which the user allowed: it is what a submodule's CMake build did
  already, and the lockfile pins the commit whose CMake files run.
- **When.** As Cargo does, any command that needs the source fetches it, and an explicit
  `dz fetch` is not needed. The editor's analysis stays offline, as it already never ran a
  CMake build (D-0090): it reads a checkout a build left in the cache, and otherwise says
  the source is not fetched yet (`DZ0008`).

**Left for later.** Telling a program its `version` has newer releases (T-0244); a
repository's own submodules, which a fetched checkout does not have; and moving the other
libraries in `examples/vendor/` off submodules, which the user asked for ImGui first.
