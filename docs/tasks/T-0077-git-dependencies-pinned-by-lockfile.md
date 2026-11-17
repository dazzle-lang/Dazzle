---
id: T-0077
title: Git dependencies, pinned by a lockfile
milestone: M16
status: done
kind: feature
area: packaging
where: `crates/dz_project/src/fetch.rs` (the lockfile, the cache, running `git`); `git_pin`, `git_checkout` and the `git` branches of `load_dependencies` and `cpp_config` in `crates/dz_project/src/lib.rs`; `load_for`'s `online`, set from `Mode` in `crates/dz_driver/src/lib.rs`; tests in `crates/dz_driver/tests/git_sources.rs`; `examples/sdl3/dazzle.toml` and its `dazzle.lock`
---

**Done 2026-11-17, the user's choices (D-0110, r142).** A dependency `ui = { git = "…",
version = "0.4.0" }` and a recipe `[cpp.libs.imgui] git = "…" version = "1.93.0"` are
fetched by `dz build`, `run` and `check` (not by the editor, `DZ0008`), each commit checked
out once into `<cache>/git/<repo>-<url hash>/<commit>/`, and pinned in `dazzle.lock` beside
the root manifest: a `version` resolves to its tag's commit, `v1.93.0` or `1.93.0`, a
`rev` is a commit in full, and a branch is refused. Point by point below: 1, with `version`
beside `rev`; 2, the commit is git's hash of the whole tree and is the content hash, and
every transitive git source goes in the root's lockfile; 3; 4, nothing fetched runs except
a recipe's CMake build, which the user allowed; 5, with Dear ImGui moved off its submodule
as the first, at the same commit the submodule had. The other libraries in
`examples/vendor/` are still submodules; newer releases of a `version` are T-0244.

What follows is the task as it was written.


`load_dependencies` accepts `{ path = … }` and says so in its own diagnostic: *"only path
dependencies are supported so far"*. A git source is a real feature, not a manifest tweak:

1. `{ git = "…", rev = "…" }`, and **`rev` is required** — a branch name is not a version,
   and resolving `master` at build time means two machines build different code. A tag is
   resolved to its commit and recorded, not trusted.
2. A **lockfile, checked in**, holding the exact commit and a content hash for every
   transitive dependency. Without it the pinning is advisory.
3. A local cache keyed on the commit, and a build that is otherwise identical to a path
   dependency's — fetched source is source.
4. Fetching and building code from a URL is the one place this toolchain would execute
   something the user did not write. Worth deciding explicitly, before implementing, what
   `dz` will and will not run from a fetched package (today: nothing — there are no build
   scripts, and `[cpp.libs]` recipes are declarative, which is a property worth keeping).
5. **A `[cpp.libs.<name>]` recipe fetches its source the same way**: the same `git` and
   `rev` (or a URL with a hash), the same lockfile and cache, with `root` then relative to
   the fetched tree. This is what retires the git submodules in `examples/vendor/` (raylib,
   RGFW): until it exists, a C library that is not in the repository has to be a
   submodule, and `git clone` without `--recursive` cannot build the examples. The user
   asked for it on 2026-10-22, when the vendored copies became submodules.
