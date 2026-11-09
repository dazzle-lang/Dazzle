---
id: T-0183
title: A release build compiles everything twice where LTO cannot link
milestone: M15
status: done
kind: feature
area: tooling
where: `crates/dz/src/build.rs`, `compile`, the `-flto=auto` fallback (`no_lto`)
---

**Done** (2026-11-09). When a release build's link fails with `-flto=auto` and the build
without it then succeeds, a file `no-lto-<key>` in the shared folder's `obj/` (T-0182)
records it, keyed by the compiler and every flag, holding the linker's first line. A later
release build, of this project or any other with the same compiler and flags, goes straight
to the build without it, and its note says which file to delete to try again. A link that
fails without it too, such as one missing a definition, is not remembered, since that says
nothing about the toolchain. Tested with a script standing in for the compiler that fails
any link with LTO (`a_toolchain_that_cannot_link_with_lto_is_remembered`, Unix only).

| Piece | Where |
|---|---|
| The fallback, and the file that remembers it | `compile` in `crates/dz/src/build.rs` (`no_lto`, `remembered`) |
| Test | `a_toolchain_that_cannot_link_with_lto_is_remembered`, `crates/dz/tests/run.rs` |

**What it is.** A release build compiles with `-flto=auto`. Where the toolchain cannot link
that, the build is tried again without it and says so. LLVM's clang targeting MSVC is one
such toolchain, since it needs `lld`. Since T-0102 the objects of both attempts are
cached, so an unchanged rebuild is quick. But a clean build, and every file an edit
reaches, is compiled twice, once for a link that is known to fail.

**What it would take.** Remember the failure: a stamp in `build/obj/dz/` naming the
compiler that could not link with `-flto=auto`. The next build skips straight to the
plain one and prints the same note. Or probe once, with a one-line program. Compiler work
only, for `docs/log/`.
