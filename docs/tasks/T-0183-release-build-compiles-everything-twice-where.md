---
id: T-0183
title: A release build compiles everything twice where LTO cannot link
milestone: M15
status: open
kind: feature
area: tooling
where: `crates/dz/src/build.rs`, `compile`, the `-flto=auto` fallback
---

**What it is.** A release build compiles with `-flto=auto`. Where the toolchain cannot link
that, the build is tried again without it and says so. LLVM's clang targeting MSVC is one
such toolchain, since it needs `lld`. Since T-0102 the objects of both attempts are
cached, so an unchanged rebuild is quick. But a clean build, and every file an edit
reaches, is compiled twice, once for a link that is known to fail.

**What it would take.** Remember the failure: a stamp in `build/obj/dz/` naming the
compiler that could not link with `-flto=auto`. The next build skips straight to the
plain one and prints the same note. Or probe once, with a one-line program. Compiler work
only, for `docs/log/`.
