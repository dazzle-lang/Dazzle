---
id: D-0068
title: Files and paths
status: decided
---

**`Path`, `File` and `Directory`, with no free functions**, chosen by the user: every operation belongs to the type it is about, and a `Path` type rather than bare `str` gives one place for joining, splitting and cross-platform separators. A `Path` always holds `/`; `\` is accepted and converted, since Windows accepts `/` and nothing else does `\`. `File` is move-only and closes on drop; whole-file `read_text`/`write_text` beside a streaming `open`. Failures are `!->` with an `IoError` code rather than typed errors (§9.2). Not yet: a `cstr` for a C API from a `Path` (`as_str` plus the owned-`cstr` row of the to-do list), file metadata such as modification times, and memory-mapped files (§14.4)
