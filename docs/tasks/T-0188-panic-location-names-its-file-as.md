---
id: T-0188
title: A panic's location names its file as the project's path was spelled on the command line
milestone: M15
status: open
kind: design
area: tooling
where: `source_location` in `crates/dz_cpp/src/lib.rs`; the paths `dz_project` gives the source map
---

**What it is.** A `SourceLocation` (`#location`, `#file`, a panic's "at" line) names its
file exactly as the project's path was given: `dz run game` prints `game/src/main.dz:13`,
`dz run /home/me/game` prints the full path, and `dz run .` from inside it `./src/main.dz`.
Spec §12.7 says only "the source file of the current call site".

**Why it matters.** Every generated file holds these strings, so building one project by two
spellings of its path changes every file and compiles every one again; T-0012 found it. The
`#line` directives had the same problem and now always name the full path (2026-11-09),
which changes nothing a program prints. This one would.

**The question for the user.** What should a location print? Rust prints the path relative
to the package (`src/main.rs:13`). Relative to the root project's directory would be the
same however `dz` was run, and short. Since it changes what programs print, it is a
decision and a §21 row, not a compiler fix.

