---
id: T-0077
title: Git dependencies, pinned by a lockfile
milestone: M16
status: open
kind: feature
area: packaging
---

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
